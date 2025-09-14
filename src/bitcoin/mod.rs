mod electrum;
mod trade_lock_script;

use crate::bitcoin::electrum::{BITCOIN_DUST_AMOUNT, ElectrsClient, UTXO};
use bitcoin::key::TapTweak;
use bitcoin::network::Network;
use bitcoin::sighash::Prevouts;
use bitcoin::{Address, key, secp256k1::PublicKey};
use bitcoin::{Amount, OutPoint, TxIn, TxOut, Txid, secp256k1};
pub use electrum::*;
use std::str::FromStr;
use std::sync::Arc;
pub use trade_lock_script::*;

pub struct Wallet {
    network: Network,
    keypair: key::Keypair,
    client: Arc<ElectrsClient>,

    pending_tx: Option<PendingTx>,
}

#[derive(Clone)]
pub struct PendingTx {
    utxos: Vec<UTXO>,
    sends: Vec<SendTo>,
    spends: Vec<Spend>,
    tx: bitcoin::Transaction,
    fee: Amount,
    fee_rate: f64,
}

#[derive(Clone, Debug)]
pub struct Spend {
    address: Address,
    utxo: UTXO,
    sequence: bitcoin::Sequence,
    leaf: bitcoin::ScriptBuf,
    merkle_root: bitcoin::taproot::TaprootSpendInfo,
    params: Vec<Option<Vec<u8>>>,
}

#[derive(Clone)]
pub struct SendTo {
    to: Address,
    amount: bitcoin::Amount,
}

impl Wallet {
    pub fn new(
        client: Arc<ElectrsClient>,
        keypair: key::Keypair,
        network: Network,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Ok(Wallet {
            network,
            keypair,
            client,
            pending_tx: None,
        })
    }

    pub async fn send(
        &mut self,
        to: String,
        amount: u64,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let to_address = bitcoin::Address::from_str(&to)?.require_network(self.network)?;
        let amount = bitcoin::Amount::from_sat(amount);
        let tx_hash = self
            .rbf(
                vec![SendTo {
                    to: to_address,
                    amount,
                }],
                vec![],
            )
            .await?;
        Ok(tx_hash.to_string())
    }

    pub async fn spend(
        &mut self,
        from: Address,
        utxo: UTXO,
        sequence: bitcoin::Sequence,
        leaf: bitcoin::ScriptBuf,
        merkle_root: bitcoin::taproot::TaprootSpendInfo,
        params: Vec<Option<Vec<u8>>>,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let spend = Spend {
            address: from,
            utxo,
            sequence,
            leaf,
            merkle_root,
            params,
        };
        let tx_hash = self.rbf(vec![], vec![spend]).await?;
        Ok(tx_hash.to_string())
    }

    pub fn address(&self) -> Address {
        let secp = secp256k1::Secp256k1::new();
        let (pubkey, _) = self.keypair.public_key().x_only_public_key();
        bitcoin::Address::p2tr(&secp, pubkey, None, self.network)
    }

    pub fn public_key(&self) -> PublicKey {
        self.keypair.public_key()
    }

    async fn get_utxos(
        &self,
        min_amount: bitcoin::Amount,
    ) -> Result<Vec<UTXO>, Box<dyn std::error::Error + Send + Sync>> {
        let utxos = self.client.get_address_utxos(&self.address().to_string())?;
        let mut selected_utxos = vec![];
        let mut required_amount = min_amount.to_sat();
        for utxo in utxos {
            if utxo.status.confirmed {
                selected_utxos.push(utxo.clone());
                if required_amount < utxo.value as u64 {
                    return Ok(selected_utxos);
                }
                required_amount -= utxo.value as u64;
            }
        }
        return Err("Insufficient funds".into());
    }

    fn sign_transaction(
        &self,
        unsigned_tx: bitcoin::Transaction,
        spends: Vec<Spend>,
        prevouts: Vec<TxOut>,
    ) -> bitcoin::Transaction {
        let secp = secp256k1::Secp256k1::new();
        let mut sighasher = bitcoin::sighash::SighashCache::new(unsigned_tx.clone());
        let prevouts = Prevouts::All(&prevouts);

        // Sign taproot script spends
        for (i, spend) in spends.iter().enumerate() {
            let sighash = sighasher
                .taproot_script_spend_signature_hash(
                    i,
                    &prevouts,
                    spend.leaf.tapscript_leaf_hash(),
                    bitcoin::TapSighashType::All,
                )
                .expect("failed to create sighash");

            let msg = bitcoin::secp256k1::Message::from(sighash);
            let sig = secp.sign_schnorr_no_aux_rand(&msg, &self.keypair);
            let signature = bitcoin::taproot::Signature {
                signature: sig,
                sighash_type: bitcoin::TapSighashType::All,
            };
            let cb = spend
                .merkle_root
                .control_block(&(spend.leaf.clone(), bitcoin::taproot::LeafVersion::TapScript))
                .unwrap();

            let mut witness = bitcoin::Witness::new();
            for param in &spend.params {
                match param {
                    Some(param) => witness.push(param.clone()),
                    None => witness.push(signature.serialize()),
                }
            }
            witness.push(spend.leaf.clone());
            witness.push(cb.serialize());
            *sighasher.witness_mut(i).unwrap() = witness;
        }

        // Sign regular taproot key spends
        for i in spends.len()..unsigned_tx.input.len() {
            let sighash = sighasher
                .taproot_key_spend_signature_hash(i, &prevouts, bitcoin::TapSighashType::All)
                .expect("failed to create sighash");

            let msg = bitcoin::secp256k1::Message::from(sighash);

            // For taproot key-path spends, we need to use the tweaked private key
            let tweaked_keypair = self.keypair.tap_tweak(&secp, None).to_keypair();
            let sig = secp.sign_schnorr_no_aux_rand(&msg, &tweaked_keypair);

            let signature = bitcoin::taproot::Signature {
                signature: sig,
                sighash_type: bitcoin::TapSighashType::All,
            };
            let mut witness = bitcoin::Witness::new();
            witness.push(signature.serialize());
            *sighasher.witness_mut(i).unwrap() = witness;
        }

        sighasher.into_transaction()
    }

    async fn fill_utxos(
        &self,
        spends: Vec<Spend>,
        sends: Vec<SendTo>,
    ) -> Result<(Vec<UTXO>, Amount), Box<dyn std::error::Error + Send + Sync>> {
        let mut spend_amount: Amount = spends
            .iter()
            .map(|spend| bitcoin::Amount::from_sat(spend.utxo.value))
            .sum();
        let mut utxos = if let Some(pending_tx) = &self.pending_tx {
            spend_amount += pending_tx
                .utxos
                .iter()
                .map(|utxo| bitcoin::Amount::from_sat(utxo.value))
                .sum();
            pending_tx.utxos.clone()
        } else {
            vec![]
        };
        let send_amount: Amount = sends.iter().map(|send| send.amount).sum();
        if send_amount > spend_amount {
            let new_utxos = self.get_utxos(send_amount - spend_amount).await?;
            spend_amount += new_utxos
                .iter()
                .map(|utxo| bitcoin::Amount::from_sat(utxo.value))
                .sum();
            utxos.extend(new_utxos);
        }
        Ok((utxos, spend_amount - send_amount))
    }

    async fn build_tx(
        &self,
        spends: Vec<Spend>,
        utxos: Vec<UTXO>,
        sends: Vec<SendTo>,
    ) -> Result<(bitcoin::Transaction, Vec<TxOut>), Box<dyn std::error::Error + Send + Sync>> {
        let mut spend_amount: Amount = spends
            .iter()
            .map(|spend| bitcoin::Amount::from_sat(spend.utxo.value))
            .sum();
        let send_amount: Amount = sends.iter().map(|send| send.amount).sum();
        spend_amount += utxos
            .iter()
            .map(|utxo| bitcoin::Amount::from_sat(utxo.value))
            .sum();

        let mut inputs: Vec<TxIn> = spends
            .iter()
            .map(|spend| TxIn {
                previous_output: OutPoint {
                    txid: Txid::from_str(&spend.utxo.txid).unwrap(),
                    vout: spend.utxo.vout,
                },
                script_sig: bitcoin::ScriptBuf::new(),
                sequence: spend.sequence,
                witness: bitcoin::Witness::new(),
            })
            .collect();
        inputs.extend(utxos.iter().map(|utxo| TxIn {
            previous_output: OutPoint {
                txid: Txid::from_str(&utxo.txid).unwrap(),
                vout: utxo.vout,
            },
            script_sig: bitcoin::ScriptBuf::new(),
            sequence: bitcoin::Sequence::ENABLE_RBF_NO_LOCKTIME,
            witness: bitcoin::Witness::new(),
        }));

        let mut prev_outputs: Vec<TxOut> = spends
            .iter()
            .map(|spend| TxOut {
                script_pubkey: spend.address.script_pubkey(),
                value: bitcoin::Amount::from_sat(spend.utxo.value),
            })
            .collect();
        prev_outputs.extend(utxos.iter().map(|utxo| TxOut {
            script_pubkey: self.address().script_pubkey(),
            value: bitcoin::Amount::from_sat(utxo.value),
        }));

        let mut outputs: Vec<TxOut> = sends
            .iter()
            .map(|send| TxOut {
                value: send.amount,
                script_pubkey: send.to.script_pubkey(),
            })
            .collect();
        let change = spend_amount - send_amount;
        let dust = bitcoin::Amount::from_sat(BITCOIN_DUST_AMOUNT);
        if change > dust {
            outputs.push(TxOut {
                value: change,
                script_pubkey: self.address().script_pubkey(),
            });
        }

        Ok((
            bitcoin::Transaction {
                version: bitcoin::blockdata::transaction::Version::TWO,
                lock_time: bitcoin::absolute::LockTime::from_height(0).unwrap(),
                input: inputs.clone(),
                output: outputs.clone(),
            },
            prev_outputs,
        ))
    }

    async fn rbf(
        &mut self,
        mut sends: Vec<SendTo>,
        mut spends: Vec<Spend>,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        // Check if there's a pending transaction
        if let Some(pending_tx) = &self.pending_tx {
            let txid = pending_tx.tx.compute_txid().to_string();

            // Check if the pending tx is confirmed
            self.pending_tx = match self.client.get_transaction_status(&txid) {
                Ok(status) => {
                    if !status.confirmed {
                        sends.extend(pending_tx.sends.clone());
                        spends.extend(pending_tx.spends.clone());
                        Some(pending_tx.clone())
                    } else {
                        None
                    }
                }
                Err(e) => {
                    return Err(format!("Error checking transaction status: {}", e).into());
                }
            };
        }

        let (mut utxos, mut change) = self.fill_utxos(spends.clone(), sends.clone()).await?;
        let (mut unsigned_tx, mut prev_outpoints) = self
            .build_tx(spends.clone(), utxos.clone(), sends.clone())
            .await?;
        let tx_sim =
            self.sign_transaction(unsigned_tx.clone(), spends.clone(), prev_outpoints.clone());

        let fee_estimate = self.client.get_fee_estimates()?;
        let tx_size = tx_sim.vsize();
        let fee_rate = fee_estimate.get("1").copied().unwrap_or(1.0);

        let mut fee = bitcoin::Amount::from_sat((tx_size as f64 * fee_rate) as u64);

        if let Some(pending_tx) = &self.pending_tx {
            if fee_rate < pending_tx.fee_rate {
                fee = bitcoin::Amount::from_sat((tx_size as f64 * pending_tx.fee_rate) as u64);
            }
            let min_fee = pending_tx.fee + Amount::from_sat(tx_size as u64);
            if min_fee > fee {
                fee = min_fee;
            }
        }
        let fee_rate = fee.to_float_in(bitcoin::Denomination::Satoshi) / tx_size as f64;

        // Handle case where change is insufficient to cover the fee
        if change <= fee {
            // Calculate additional amount needed
            let additional_amount_needed =
                fee - change + bitcoin::Amount::from_sat(BITCOIN_DUST_AMOUNT);
            let new_utxos = self.get_utxos(additional_amount_needed).await?;
            utxos.extend(new_utxos);

            // Rebuild transaction with new UTXOs
            let (new_unsigned_tx, new_prev_outpoints) = self
                .build_tx(spends.clone(), utxos.clone(), sends.clone())
                .await?;
            unsigned_tx = new_unsigned_tx;
            prev_outpoints = new_prev_outpoints;

            // Recalculate change with new UTXOs
            let spend_amount: Amount = spends
                .iter()
                .map(|spend| bitcoin::Amount::from_sat(spend.utxo.value))
                .sum();
            let utxo_amount: Amount = utxos
                .iter()
                .map(|utxo| bitcoin::Amount::from_sat(utxo.value))
                .sum();
            let send_amount: Amount = sends.iter().map(|send| send.amount).sum();
            change = spend_amount + utxo_amount - send_amount;

            if change <= fee {
                return Err(
                    "Insufficient funds to cover transaction fee even after adding UTXOs".into(),
                );
            }
        }

        // Adjust change output to account for fee
        let change_index = unsigned_tx.output.len() - 1;
        let mut change_output = unsigned_tx.output[change_index].clone();
        change_output.value = change - fee;
        unsigned_tx.output[change_index] = change_output;

        let signed_tx =
            self.sign_transaction(unsigned_tx.clone(), spends.clone(), prev_outpoints.clone());
        self.pending_tx = Some(PendingTx {
            utxos,
            sends,
            spends,
            tx: signed_tx.clone(),
            fee,
            fee_rate,
        });
        dbg!(hex::encode(bitcoin::consensus::serialize(&signed_tx)));
        let tx_hash = self.client.broadcast_transaction(&signed_tx)?;
        Ok(tx_hash)
    }
}
