use crate::models::Swap;
use bitcoin::{
    Address, Network, ScriptBuf, XOnlyPublicKey,
    key::Secp256k1,
    opcodes,
    script::Builder,
    secp256k1::{PublicKey, SecretKey},
    taproot::{TaprootBuilder, TaprootSpendInfo},
};
use eyre::{Result, eyre};
use once_cell::sync::Lazy;
use sha2::{Digest, Sha256};
use std::str::FromStr;

/// Static secp256k1 context for cryptographic operations.
static SECP: Lazy<Secp256k1<bitcoin::secp256k1::All>> = Lazy::new(Secp256k1::new);

/// NUMS (Nothing-Up-My-Sleeve) internal key for Taproot HTLC addresses.
///
/// Generates a deterministic, provably uncontrollable internal key per BIP-341:
/// 1. Computes SHA256("TradelockHTLC") as scalar r.
/// 2. Uses BIP-341's H point (fixed point on secp256k1 curve).
/// 3. Computes r*G + H to derive the final public key.
/// 4. Converts to x-only format (32-byte x-coordinate, even y-parity).
pub static NUMS_INTERNAL_KEY: Lazy<XOnlyPublicKey> = Lazy::new(|| {
    // Step 1: Hash "TradelockHTLC" to get scalar r
    let r = Sha256::digest(b"TradelockHTLC");

    // Step 2: Parse BIP-341 H point (fixed point for NUMS key)
    const H_HEX: &str = "0250929b74c1a04954b78b4b6035e97a5e078a5a0f28ec96d547bfee9ace803ac0";
    let h_bytes = hex::decode(H_HEX).expect("Invalid hex for BIP-341 H point");
    let h = PublicKey::from_slice(&h_bytes).expect("Invalid BIP-341 H point");

    // Step 3: Compute r * G (point multiplication with generator)
    let r_scalar = SecretKey::from_slice(&r).expect("Invalid scalar from SHA256 digest");
    let r_g = PublicKey::from_secret_key(&*SECP, &r_scalar);

    // Step 4: Combine H + r*G
    let nums = h.combine(&r_g).expect("Point addition failed for NUMS key");

    // Step 5: Convert to x-only public key (32-byte x-coordinate)
    let (xonly, _) = nums.x_only_public_key();
    xonly
});

/// Configuration for a Taproot-based trade lock
pub struct TradeLockConfig {
    pub alice_pubkey: XOnlyPublicKey,
    pub bob_pubkey: XOnlyPublicKey,
    pub network: Network,
}

impl TradeLockConfig {
    /// Creates a new trade lock configuration with Alice and Bob's public keys and network.
    pub fn new(network: Network, alice_pubkey: &str, bob_pubkey: &str) -> Result<Self> {
        let alice_pubkey = XOnlyPublicKey::from_str(alice_pubkey)
            .map_err(|e| eyre!("Invalid Alice public key: {}", e))?;
        let bob_pubkey = XOnlyPublicKey::from_str(bob_pubkey)
            .map_err(|e| eyre!("Invalid Bob public key: {}", e))?;
        Ok(Self {
            alice_pubkey,
            bob_pubkey,
            network,
        })
    }

    /// Generates a trade ID by hashing Alice and Bob's public keys.
    pub fn trade_id(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.alice_pubkey.to_string().as_bytes());
        hasher.update(self.bob_pubkey.to_string().as_bytes());
        hex::encode(hasher.finalize())
    }

    /// Builds a 2-of-2 multisig script for the Taproot script path (trade completion).
    fn multisig_leaf_script(
        alice_pubkey: &XOnlyPublicKey,
        bob_pubkey: &XOnlyPublicKey,
    ) -> ScriptBuf {
        Builder::new()
            .push_slice(&alice_pubkey.serialize())
            .push_opcode(opcodes::all::OP_CHECKSIG)
            .push_slice(&bob_pubkey.serialize())
            .push_opcode(opcodes::all::OP_CHECKSIGADD)
            .push_int(2)
            .push_opcode(opcodes::all::OP_NUMEQUAL)
            .into_script()
    }

    /// Builds a single-key signature check script for the Taproot script path (e.g., refund).
    fn signature_check_script(pubkey: &XOnlyPublicKey) -> ScriptBuf {
        Builder::new()
            .push_slice(&pubkey.serialize())
            .push_opcode(opcodes::all::OP_CHECKSIG)
            .into_script()
    }

    /// Constructs Taproot spend info with a multisig leaf and a refund leaf.
    fn build_taproot_spend_info(
        owner: &XOnlyPublicKey,
        recipient: &XOnlyPublicKey,
    ) -> Result<TaprootSpendInfo> {
        let multisig_leaf = Self::multisig_leaf_script(owner, recipient);
        let signature_leaf = Self::signature_check_script(owner);

        let mut taproot_builder = TaprootBuilder::new();
        // Add multisig leaf at depth 1 for trade completion
        taproot_builder = taproot_builder
            .add_leaf(1, multisig_leaf)
            .map_err(|e| eyre!("Failed to add multisig leaf to Taproot tree: {}", e))?;
        // Add refund leaf at depth 1 for owner refund
        taproot_builder = taproot_builder
            .add_leaf(1, signature_leaf)
            .map_err(|e| eyre!("Failed to add refund leaf to Taproot tree: {}", e))?;

        if !taproot_builder.is_finalizable() {
            return Err(eyre!("Taproot builder is not in a finalizable state"));
        }

        let internal_key = *NUMS_INTERNAL_KEY;
        taproot_builder
            .finalize(&*SECP, internal_key)
            .map_err(|_| eyre!("Failed to finalize Taproot spend info"))
    }

    /// Generates Alice's Taproot address for the trade lock.
    pub fn alice_taproot_address(&self) -> Result<Address> {
        let spend_info = Self::build_taproot_spend_info(&self.alice_pubkey, &self.bob_pubkey)
            .map_err(|e| eyre!("Failed to create Alice's Taproot spend info: {}", e))?;
        Ok(Address::p2tr(
            &*SECP,
            spend_info.internal_key(),
            spend_info.merkle_root(),
            self.network,
        ))
    }

    /// Generates Bob's Taproot address for the trade lock.
    pub fn bob_taproot_address(&self) -> Result<Address> {
        let spend_info = Self::build_taproot_spend_info(&self.bob_pubkey, &self.alice_pubkey)
            .map_err(|e| eyre!("Failed to create Bob's Taproot spend info: {}", e))?;
        Ok(Address::p2tr(
            &*SECP,
            spend_info.internal_key(),
            spend_info.merkle_root(),
            self.network,
        ))
    }
}

impl TryInto<Swap> for TradeLockConfig {
    type Error = eyre::Report;

    /// Converts the trade lock configuration into a Swap struct.
    fn try_into(self) -> std::result::Result<Swap, eyre::Report> {
        let alice_addr = self.alice_taproot_address()?;
        let bob_addr = self.bob_taproot_address()?;
        Ok(Swap::new(
            self.alice_pubkey.to_string(),
            self.bob_pubkey.to_string(),
            alice_addr.to_string(),
            bob_addr.to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::{Network, XOnlyPublicKey};
    use eyre::Result;
    use std::str::FromStr;

    // Helper function to generate valid XOnlyPublicKey for testing
    fn generate_test_keypair() -> (String, String) {
        let secp = Secp256k1::new();
        let secret_key = SecretKey::from_slice(&[0x42; 32]).unwrap();
        let public_key = PublicKey::from_secret_key(&secp, &secret_key);
        let (xonly, _) = public_key.x_only_public_key();
        let another_secret_key = SecretKey::from_slice(&[0x43; 32]).unwrap();
        let another_public_key = PublicKey::from_secret_key(&secp, &another_secret_key);
        let (another_xonly, _) = another_public_key.x_only_public_key();
        (xonly.to_string(), another_xonly.to_string())
    }

    #[test]
    fn test_new_tradelock_config_valid() {
        let (alice_pubkey, bob_pubkey) = generate_test_keypair();
        let config = TradeLockConfig::new(Network::Testnet, &alice_pubkey, &bob_pubkey);
        assert!(config.is_ok());
        let config = config.unwrap();
        assert_eq!(config.network, Network::Testnet);
        assert_eq!(config.alice_pubkey.to_string(), alice_pubkey);
        assert_eq!(config.bob_pubkey.to_string(), bob_pubkey);
    }

    #[test]
    fn test_new_tradelock_config_invalid_pubkeys() {
        let invalid_pubkey = "invalid_pubkey";
        let valid_pubkey = generate_test_keypair().0;

        // Test with invalid Alice pubkey
        let result = TradeLockConfig::new(Network::Testnet, invalid_pubkey, &valid_pubkey);
        assert!(result.is_err());
        assert!(
            result
                .err()
                .unwrap()
                .to_string()
                .contains("Invalid Alice public key")
        );

        // Test with invalid Bob pubkey
        let result = TradeLockConfig::new(Network::Testnet, &valid_pubkey, invalid_pubkey);
        assert!(result.is_err());
        assert!(
            result
                .err()
                .unwrap()
                .to_string()
                .contains("Invalid Bob public key")
        );
    }

    #[test]
    fn test_trade_id_deterministic() {
        let (alice_pubkey, bob_pubkey) = generate_test_keypair();
        let config1 = TradeLockConfig::new(Network::Testnet, &alice_pubkey, &bob_pubkey).unwrap();
        let config2 = TradeLockConfig::new(Network::Testnet, &alice_pubkey, &bob_pubkey).unwrap();

        let trade_id1 = config1.trade_id();
        let trade_id2 = config2.trade_id();
        assert_eq!(trade_id1, trade_id2, "Trade IDs should be deterministic");
        assert_eq!(
            trade_id1.len(),
            64,
            "Trade ID should be 64 hex chars (SHA256)"
        );
    }

    #[test]
    fn test_multisig_leaf_script() {
        let (alice_pubkey_str, bob_pubkey_str) = generate_test_keypair();
        let alice_pubkey = XOnlyPublicKey::from_str(&alice_pubkey_str).unwrap();
        let bob_pubkey = XOnlyPublicKey::from_str(&bob_pubkey_str).unwrap();
        let script = TradeLockConfig::multisig_leaf_script(&alice_pubkey, &bob_pubkey);

        dbg!(&script.to_asm_string());
        // Check script structure: <32-byte push> <pubkey1> OP_CHECKSIG <32-byte push> <pubkey2> OP_CHECKSIGADD 2 OP_NUMEQUAL
        let script_bytes = script.as_bytes();
        assert_eq!(
            script_bytes.len(),
            70,
            "Expected multisig script length: 70 bytes"
        );
        assert_eq!(
            script_bytes[0],
            0x20, // 32-byte push opcode for Alice pubkey
            "Expected 0x20 push opcode for Alice pubkey"
        );
        assert_eq!(
            script_bytes[1..33],
            alice_pubkey.serialize(),
            "Incorrect Alice pubkey"
        );
        assert_eq!(
            script_bytes[33],
            opcodes::all::OP_CHECKSIG.to_u8(),
            "Expected OP_CHECKSIG"
        );
        assert_eq!(
            script_bytes[34],
            0x20, // 32-byte push opcode for Bob pubkey
            "Expected 0x20 push opcode for Bob pubkey"
        );
        assert_eq!(
            script_bytes[35..67],
            bob_pubkey.serialize(),
            "Incorrect Bob pubkey"
        );
        assert_eq!(
            script_bytes[67],
            opcodes::all::OP_CHECKSIGADD.to_u8(),
            "Expected OP_CHECKSIGADD"
        );
        assert_eq!(
            script_bytes[68],
            opcodes::all::OP_PUSHNUM_2.to_u8(),
            "Expected 2 for multisig threshold"
        );
        assert_eq!(
            script_bytes[69],
            opcodes::all::OP_NUMEQUAL.to_u8(),
            "Expected OP_NUMEQUAL"
        );
    }

    #[test]
    fn test_signature_check_script() {
        let (alice_pubkey_str, _) = generate_test_keypair();
        let alice_pubkey = XOnlyPublicKey::from_str(&alice_pubkey_str).unwrap();
        let script = TradeLockConfig::signature_check_script(&alice_pubkey);

        // Check script structure: <pubkey> OP_CHECKSIG
        let script_bytes = script.as_bytes();
        assert_eq!(
            script_bytes.len(),
            34,
            "Expected signature check script length: 34 bytes"
        );
        assert_eq!(
            script_bytes[0],
            0x20, // 32-byte push opcode
            "Expected 0x20 push opcode for 32-byte pubkey"
        );
        assert_eq!(
            script_bytes[1..33],
            alice_pubkey.serialize(),
            "Incorrect pubkey"
        );
        assert_eq!(
            script_bytes[33],
            opcodes::all::OP_CHECKSIG.to_u8(),
            "Expected OP_CHECKSIG"
        );
    }

    #[test]
    fn test_build_taproot_spend_info() {
        let (alice_pubkey_str, bob_pubkey_str) = generate_test_keypair();
        let alice_pubkey = XOnlyPublicKey::from_str(&alice_pubkey_str).unwrap();
        let bob_pubkey = XOnlyPublicKey::from_str(&bob_pubkey_str).unwrap();
        let spend_info = TradeLockConfig::build_taproot_spend_info(&alice_pubkey, &bob_pubkey);
        assert!(spend_info.is_ok(), "Failed to build Taproot spend info");
        let spend_info = spend_info.unwrap();
        assert_eq!(
            spend_info.internal_key(),
            *NUMS_INTERNAL_KEY,
            "Incorrect internal key"
        );
        assert!(spend_info.merkle_root().is_some(), "Expected Merkle root");
    }

    #[test]
    fn test_alice_taproot_address() {
        let (alice_pubkey, bob_pubkey) = generate_test_keypair();
        let config = TradeLockConfig::new(Network::Testnet, &alice_pubkey, &bob_pubkey).unwrap();
        let address = config.alice_taproot_address();
        assert!(
            address.is_ok(),
            "Failed to generate Alice's Taproot address"
        );
        let address = address.unwrap();
        assert!(
            address.to_string().starts_with("tb1"),
            "Address invalid for Testnet"
        );
        assert!(
            address.to_string().starts_with("tb1p"),
            "Expected Testnet Taproot address prefix"
        );
    }

    #[test]
    fn test_bob_taproot_address() {
        let (alice_pubkey, bob_pubkey) = generate_test_keypair();
        let config = TradeLockConfig::new(Network::Testnet, &alice_pubkey, &bob_pubkey).unwrap();
        let address = config.bob_taproot_address();
        assert!(address.is_ok(), "Failed to generate Bob's Taproot address");
        let address = address.unwrap();
        assert!(
            address.to_string().starts_with("tb1"),
            "Address invalid for Testnet"
        );
        assert!(
            address.to_string().starts_with("tb1p"),
            "Expected Testnet Taproot address prefix"
        );
    }

    #[test]
    fn test_try_into_swap() {
        let (alice_pubkey, bob_pubkey) = generate_test_keypair();
        let config = TradeLockConfig::new(Network::Testnet, &alice_pubkey, &bob_pubkey).unwrap();
        let swap: Result<Swap> = config.try_into();
        assert!(swap.is_ok(), "Failed to convert TradeLockConfig to Swap");
        let swap = swap.unwrap();
        assert_eq!(
            swap.alice_pubkey.to_string(),
            alice_pubkey,
            "Incorrect Alice pubkey in Swap"
        );
        assert_eq!(
            swap.bob_pubkey.to_string(),
            bob_pubkey,
            "Incorrect Bob pubkey in Swap"
        );
        assert!(
            swap.alice_wallet_addr.starts_with("tb1p"),
            "Expected Testnet Taproot address for Alice"
        );
        assert!(
            swap.bob_wallet_addr.starts_with("tb1p"),
            "Expected Testnet Taproot address for Bob"
        );
    }
}
#[cfg(test)]
mod tx_test {
    use super::*;
    use crate::bitcoin::electrum::{ElectrsClient, UTXO};
    use crate::bitcoin::{Merry, SendTo, Spend, Wallet};
    use bitcoin::secp256k1::{PublicKey, Secp256k1, SecretKey};
    use bitcoin::sighash::Prevouts;
    use bitcoin::{
        Amount, Network, OutPoint, ScriptBuf, Sequence, TxOut, Txid, XOnlyPublicKey,
        absolute::LockTime, blockdata::transaction::Version,
    };
    use eyre::Result;
    use rand::RngCore;
    use std::str::FromStr;
    use std::sync::Arc;
    use std::time::Duration;

    // Test configuration constants
    const REGTEST_ESPLORA_URL: &str = "http://0.0.0.0:30000";
    const REGTEST_BITCOIN_RPC_URL: &str = "http://0.0.0.0:18443";
    const FUNDING_WAIT: Duration = Duration::from_secs(5);

    /// Test fixture containing keypairs, wallets, and configuration
    struct TestFixture {
        alice_secret: SecretKey,
        bob_secret: SecretKey,
        alice_pubkey: String,
        bob_pubkey: String,
        config: TradeLockConfig,
        alice_wallet: Wallet,
        bob_wallet: Wallet,
        merry: Merry,
    }

    /// Creates a test fixture with initialized keypairs, wallets, and configuration
    fn setup_test_fixture() -> Result<TestFixture> {
        let secp = Secp256k1::new();
        let mut rng = rand::thread_rng();

        // Generate keypairs
        let mut alice_seed = [0u8; 32];
        rng.fill_bytes(&mut alice_seed);
        let alice_secret = SecretKey::from_slice(&alice_seed)?;
        let alice_pubkey = PublicKey::from_secret_key(&secp, &alice_secret)
            .x_only_public_key()
            .0
            .to_string();

        let mut bob_seed = [0u8; 32];
        rng.fill_bytes(&mut bob_seed);
        let bob_secret = SecretKey::from_slice(&bob_seed)?;
        let bob_pubkey = PublicKey::from_secret_key(&secp, &bob_secret)
            .x_only_public_key()
            .0
            .to_string();

        // Create TradeLock configuration
        let config = TradeLockConfig::new(Network::Regtest, &alice_pubkey, &bob_pubkey)?;

        // Create wallets
        let alice_keypair = bitcoin::key::Keypair::from_secret_key(&secp, &alice_secret);
        let bob_keypair = bitcoin::key::Keypair::from_secret_key(&secp, &bob_secret);

        let merry = Merry::new();
        let alice_wallet = Wallet::new(
            Arc::new(
                ElectrsClient::new(REGTEST_ESPLORA_URL.to_string()).map_err(|e| eyre::eyre!(e))?,
            ),
            alice_keypair,
            Network::Regtest,
        )
        .map_err(|e| eyre::eyre!(e))?;
        let bob_wallet = Wallet::new(
            Arc::new(
                ElectrsClient::new(REGTEST_ESPLORA_URL.to_string()).map_err(|e| eyre::eyre!(e))?,
            ),
            bob_keypair,
            Network::Regtest,
        )
        .map_err(|e| eyre::eyre!(e))?;

        Ok(TestFixture {
            alice_secret,
            bob_secret,
            alice_pubkey,
            bob_pubkey,
            config,
            alice_wallet,
            bob_wallet,
            merry,
        })
    }

    /// Funds an address and waits for confirmation
    async fn fund_address(merry: &Merry, address: &str) -> Result<Vec<UTXO>> {
        merry.fund(address);
        std::thread::sleep(FUNDING_WAIT);
        let utxos = merry
            .client()
            .get_address_utxos(address)
            .map_err(|e| eyre::eyre!(e))?;
        if utxos.is_empty() {
            Err(eyre::eyre!("No UTXOs found for address: {}", address))
        } else {
            Ok(utxos)
        }
    }

    #[tokio::test]
    async fn test_fund_tradelock_addresses() -> Result<()> {
        let fixture = setup_test_fixture()?;
        println!(
            "[DEBUG] Alice pubkey: {}, Bob pubkey: {}",
            fixture.alice_pubkey, fixture.bob_pubkey
        );

        // Fund Alice's Taproot address
        let alice_addr = fixture.config.alice_taproot_address()?;
        println!("[DEBUG] Funding Alice's Taproot address: {}", alice_addr);
        let alice_utxos = fund_address(&fixture.merry, &alice_addr.to_string()).await?;
        println!("[DEBUG] Alice's UTXOs: {:?}", alice_utxos);
        assert!(
            !alice_utxos.is_empty(),
            "Alice's Taproot address should have UTXOs"
        );

        // Fund Bob's Taproot address
        let bob_addr = fixture.config.bob_taproot_address()?;
        println!("[DEBUG] Funding Bob's Taproot address: {}", bob_addr);
        let bob_utxos = fund_address(&fixture.merry, &bob_addr.to_string()).await?;
        println!("[DEBUG] Bob's UTXOs: {:?}", bob_utxos);
        assert!(
            !bob_utxos.is_empty(),
            "Bob's Taproot address should have UTXOs"
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_multisig_spend_transaction() -> Result<()> {
        let fixture = setup_test_fixture()?;
        println!(
            "[DEBUG] Alice pubkey: {}, Bob pubkey: {}",
            fixture.alice_pubkey, fixture.bob_pubkey
        );

        // Fund wallets
        fund_address(&fixture.merry, &fixture.alice_wallet.address().to_string()).await?;
        fund_address(&fixture.merry, &fixture.bob_wallet.address().to_string()).await?;

        // Fund Alice's Taproot address
        let alice_addr = fixture.config.alice_taproot_address()?;
        println!("[DEBUG] Funding Alice's Taproot address: {}", alice_addr);
        let utxos = fund_address(&fixture.merry, &alice_addr.to_string()).await?;
        let utxo = utxos[0].clone();
        println!("[DEBUG] Using UTXO: {:?}", utxo);

        // Create multisig leaf script
        let alice_xonly = XOnlyPublicKey::from_str(&fixture.alice_pubkey)?;
        let bob_xonly = XOnlyPublicKey::from_str(&fixture.bob_pubkey)?;
        let multisig_script = TradeLockConfig::multisig_leaf_script(&alice_xonly, &bob_xonly);
        println!("[DEBUG] Multisig script: {:?}", multisig_script);

        // Build Taproot spend info
        let spend_info = TradeLockConfig::build_taproot_spend_info(&alice_xonly, &bob_xonly)?;
        println!("[DEBUG] Taproot spend info: {:?}", spend_info);

        // Create unsigned transaction
        let unsigned_tx = bitcoin::Transaction {
            version: Version::TWO,
            lock_time: LockTime::from_height(0)?,
            input: vec![bitcoin::TxIn {
                previous_output: OutPoint::new(Txid::from_str(&utxo.txid)?, utxo.vout),
                script_sig: ScriptBuf::new(),
                sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
                witness: bitcoin::Witness::new(),
            }],
            output: vec![TxOut {
                value: Amount::from_sat(99999838), // Subtract fee
                script_pubkey: fixture.alice_wallet.address().script_pubkey(),
            }],
        };
        println!("[DEBUG] Unsigned transaction: {:#?}", unsigned_tx);

        // Generate sighash
        let sighash = bitcoin::sighash::SighashCache::new(&unsigned_tx)
            .taproot_script_spend_signature_hash(
                0,
                &Prevouts::All(&[TxOut {
                    value: Amount::from_sat(utxo.value),
                    script_pubkey: alice_addr.script_pubkey(),
                }]),
                multisig_script.tapscript_leaf_hash(),
                bitcoin::TapSighashType::Default,
            )?;
        println!("[DEBUG] Sighash: {}", sighash);

        // Sign transaction
        let secp = Secp256k1::new();
        let msg = bitcoin::secp256k1::Message::from(sighash);
        let alice_keypair = bitcoin::key::Keypair::from_secret_key(&secp, &fixture.alice_secret);
        let alice_sig = secp.sign_schnorr_no_aux_rand(&msg, &alice_keypair);
        let bob_keypair = bitcoin::key::Keypair::from_secret_key(&secp, &fixture.bob_secret);
        let bob_sig = secp.sign_schnorr_no_aux_rand(&msg, &bob_keypair);

        // Create witness
        let mut sighasher = bitcoin::sighash::SighashCache::new(unsigned_tx);
        let cb = spend_info
            .control_block(&(
                multisig_script.clone(),
                bitcoin::taproot::LeafVersion::TapScript,
            ))
            .ok_or_else(|| eyre::eyre!("Failed to create control block"))?;

        let mut witness = bitcoin::Witness::new();
        for param in &[bob_sig.serialize().to_vec(), alice_sig.serialize().to_vec()] {
            witness.push(param.clone());
        }
        witness.push(multisig_script.clone());
        witness.push(cb.serialize());
        *sighasher
            .witness_mut(0)
            .ok_or_else(|| eyre::eyre!("Failed to get witness"))? = witness;

        // Broadcast transaction
        let signed_tx = sighasher.into_transaction();
        let tx_hex = hex::encode(bitcoin::consensus::serialize(&signed_tx));
        println!("[DEBUG] Signed transaction hex: {}", tx_hex);
        let tx_hash = fixture
            .merry
            .client()
            .broadcast_transaction(&signed_tx)
            .map_err(|e| eyre::eyre!(e))?;
        println!("[DEBUG] Broadcasted tx hash: {}", tx_hash);

        // Verify transaction
        assert_eq!(tx_hash.len(), 64, "Transaction hash should be 64 hex chars");
        let status = fixture
            .merry
            .client()
            .get_transaction_status(&tx_hash)
            .map_err(|e| eyre::eyre!(e))?;
        println!("[DEBUG] Transaction status: {:?}", status);
        assert!(!status.confirmed, "Transaction should be in mempool");

        Ok(())
    }

    #[tokio::test]
    async fn test_single_sig_refund_transaction() -> Result<()> {
        let mut fixture = setup_test_fixture()?;
        println!(
            "[LOG] Alice pubkey: {}, Bob pubkey: {}",
            fixture.alice_pubkey, fixture.bob_pubkey
        );

        // Fund Alice's wallet
        fund_address(&fixture.merry, &fixture.alice_wallet.address().to_string()).await?;
        fixture.merry.mine();
        std::thread::sleep(FUNDING_WAIT);

        // Fund Alice's Taproot address
        let alice_addr = fixture.config.alice_taproot_address()?;
        println!("[LOG] Alice's Taproot address: {}", alice_addr);
        fund_address(&fixture.merry, &alice_addr.to_string()).await?;
        fixture.merry.mine();
        std::thread::sleep(FUNDING_WAIT);

        // Get UTXOs
        let utxos = fixture
            .merry
            .client()
            .get_address_utxos(&alice_addr.to_string())
            .map_err(|e| eyre::eyre!(e))?;
        println!(
            "[LOG] Retrieved UTXOs for Alice's Taproot address: {:?}",
            utxos
        );
        assert!(!utxos.is_empty(), "No UTXOs found for Alice's address");
        let utxo = utxos[0].clone();
        println!("[LOG] Selected UTXO: {:?}", utxo);

        // Create refund script
        let alice_xonly = XOnlyPublicKey::from_str(&fixture.alice_pubkey)?;
        let refund_script = TradeLockConfig::signature_check_script(&alice_xonly);
        println!(
            "[LOG] Created single-sig refund script: {:?}",
            refund_script
        );

        // Build Taproot spend info
        let spend_info = TradeLockConfig::build_taproot_spend_info(
            &alice_xonly,
            &XOnlyPublicKey::from_str(&fixture.bob_pubkey)?,
        )?;
        println!("[LOG] Built Taproot spend info: {:?}", spend_info);

        // Create spend
        let spend = Spend {
            address: alice_addr.clone(),
            utxo,
            sequence: Sequence::ENABLE_LOCKTIME_NO_RBF,
            leaf: refund_script,
            merkle_root: spend_info,
            params: vec![None],
        };
        println!("[LOG] Created Spend object: {:?}", spend);

        // Broadcast transaction
        let tx_hash = fixture
            .alice_wallet
            .rbf(vec![], vec![spend])
            .await
            .map_err(|e| eyre::eyre!(e))?;
        println!("[LOG] Broadcasted transaction. Tx hash: {}", tx_hash);
        let status = fixture
            .merry
            .client()
            .get_transaction_status(&tx_hash)
            .map_err(|e| eyre::eyre!(e))?;
        println!("[LOG] Transaction status: {:?}", status);
        assert!(!status.confirmed, "Transaction should be in mempool");

        Ok(())
    }
}
