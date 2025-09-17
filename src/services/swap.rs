use crate::{
    btc::{ElectrsClient, TradeLockConfig},
    models::Swap,
    storage::Storage,
};
use bitcoin::{Network, XOnlyPublicKey};
use eyre::{Result, eyre};
use sha2::{Digest, Sha256};
use std::{str::FromStr, sync::Arc};

/// SwapManager handles the creation and management of swap accounts
pub struct SwapManager {
    /// Electrum client for Bitcoin network operations
    pub electrum_client: ElectrsClient,
    /// Cosigner's public key for trade lock scripts
    pub cosigner_pubkey: XOnlyPublicKey,
    /// Storage backend for persisting swaps
    pub store: Arc<dyn Storage + Send + Sync>,
    /// Bitcoin network (mainnet, testnet, regtest)
    pub network: Network,
}

impl SwapManager {
    /// Creates a new SwapManager instance
    pub fn new(
        electrum_client: ElectrsClient,
        cosigner_pubkey: &str,
        store: Arc<dyn Storage + Send + Sync>,
        network: Network,
    ) -> Result<Self> {
        let cosigner_pubkey = XOnlyPublicKey::from_str(cosigner_pubkey)
            .map_err(|e| eyre!("Invalid cosigner public key: {}", e))?;

        Ok(Self {
            electrum_client,
            cosigner_pubkey,
            store,
            network,
        })
    }

    /// Creates new swap accounts for Alice and Bob
    ///
    /// This function:
    /// 1. Converts string representations of Alice and Bob's keys to XOnlyPublicKey
    /// 2. Uses TradeLockConfig to create two accounts for Alice and Bob
    /// 3. Generates a swap_id using SHA256(alice_pubkey, bob_pubkey)
    /// 4. Checks if accounts already exist in cache before creating new ones
    /// 5. Stores the swap using the storage interface
    pub async fn new_swap_accounts(&self, alice_key: &str, bob_key: &str) -> Result<Swap> {
        // Convert string keys to XOnlyPublicKey
        let alice_pubkey = XOnlyPublicKey::from_str(alice_key)
            .map_err(|e| eyre!("Invalid Alice public key: {}", e))?;
        let bob_pubkey = XOnlyPublicKey::from_str(bob_key)
            .map_err(|e| eyre!("Invalid Bob public key: {}", e))?;

        // Generate swap_id using SHA256(alice_pubkey, bob_pubkey)
        let swap_id = self.generate_swap_id(&alice_pubkey, &bob_pubkey);

        // Check if swap already exists in cache
        if let Some(existing_swap) = self.store.get_swap(&swap_id).await? {
            return Ok(existing_swap);
        }

        // Create TradeLockConfig for the swap
        let trade_lock_config = TradeLockConfig::new(
            self.network,
            alice_key,
            bob_key,
            &self.cosigner_pubkey.to_string(),
        )?;

        // Convert TradeLockConfig to Swap
        let swap: Swap = trade_lock_config.try_into()?;

        // Store the swap
        self.store.store_swap(&swap_id, &swap).await?;

        Ok(swap)
    }

    /// Generates a deterministic swap ID using SHA256 hash of Alice and Bob's public keys
    fn generate_swap_id(
        &self,
        alice_pubkey: &XOnlyPublicKey,
        bob_pubkey: &XOnlyPublicKey,
    ) -> String {
        let mut hasher = Sha256::new();
        hasher.update(alice_pubkey.to_string().as_bytes());
        hasher.update(bob_pubkey.to_string().as_bytes());
        hex::encode(hasher.finalize())
    }

    /// Retrieves a swap by its ID
    pub async fn get_swap(&self, swap_id: &str) -> Result<Option<Swap>> {
        self.store.get_swap(swap_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::MokaStrore;
    use bitcoin::Network;

    // Helper function to generate valid XOnlyPublicKey for testing
    fn generate_test_keypair() -> (String, String, String) {
        use bitcoin::{
            key::Secp256k1,
            secp256k1::{PublicKey, SecretKey},
        };

        let secp = Secp256k1::new();
        let secret_key = SecretKey::from_slice(&[0x42; 32]).unwrap();
        let public_key = PublicKey::from_secret_key(&secp, &secret_key);
        let (xonly, _) = public_key.x_only_public_key();
        let another_secret_key = SecretKey::from_slice(&[0x43; 32]).unwrap();
        let another_public_key = PublicKey::from_secret_key(&secp, &another_secret_key);
        let (another_xonly, _) = another_public_key.x_only_public_key();
        let cosigner_secret_key = SecretKey::from_slice(&[0x44; 32]).unwrap();
        let cosigner_public_key = PublicKey::from_secret_key(&secp, &cosigner_secret_key);
        let (cosigner_xonly, _) = cosigner_public_key.x_only_public_key();
        (
            xonly.to_string(),
            another_xonly.to_string(),
            cosigner_xonly.to_string(),
        )
    }

    #[tokio::test]
    async fn test_new_swap_accounts() {
        let (alice_pubkey, bob_pubkey, cosigner_pubkey) = generate_test_keypair();

        // Create mock electrum client
        let electrum_client = ElectrsClient::new("http://localhost:30000".to_string())
            .expect("Failed to create ElectrsClient");

        // Create storage
        let store = Arc::new(MokaStrore::new());

        // Create SwapManager
        let swap_manager =
            SwapManager::new(electrum_client, &cosigner_pubkey, store, Network::Testnet)
                .expect("Failed to create SwapManager");

        // Test creating new swap accounts
        let swap = swap_manager
            .new_swap_accounts(&alice_pubkey, &bob_pubkey)
            .await
            .expect("Failed to create swap accounts");

        // Verify swap properties
        assert_eq!(swap.alice_pubkey, alice_pubkey);
        assert_eq!(swap.bob_pubkey, bob_pubkey);
        assert!(!swap.alice_wallet_addr.is_empty());
        assert!(!swap.bob_wallet_addr.is_empty());
        assert!(!swap.swap_id.is_empty());
        assert_eq!(swap.challenges.len(), 0);

        // Test that calling again returns the same swap (cached)
        let cached_swap = swap_manager
            .new_swap_accounts(&alice_pubkey, &bob_pubkey)
            .await
            .expect("Failed to get cached swap");
        assert_eq!(swap.swap_id, cached_swap.swap_id);
    }

    #[tokio::test]
    async fn test_swap_id_deterministic() {
        let (alice_pubkey, bob_pubkey, cosigner_pubkey) = generate_test_keypair();

        let electrum_client = ElectrsClient::new("http://localhost:30000".to_string())
            .expect("Failed to create ElectrsClient");
        let store = Arc::new(MokaStrore::new());

        let swap_manager =
            SwapManager::new(electrum_client, &cosigner_pubkey, store, Network::Testnet)
                .expect("Failed to create SwapManager");

        // Create two swaps with the same keys
        let swap1 = swap_manager
            .new_swap_accounts(&alice_pubkey, &bob_pubkey)
            .await
            .expect("Failed to create first swap");
        let swap2 = swap_manager
            .new_swap_accounts(&alice_pubkey, &bob_pubkey)
            .await
            .expect("Failed to create second swap");

        // Swap IDs should be identical
        assert_eq!(swap1.swap_id, swap2.swap_id);
    }

    #[tokio::test]
    async fn test_invalid_public_keys() {
        let (alice_pubkey, bob_pubkey, cosigner_pubkey) = generate_test_keypair();

        let electrum_client = ElectrsClient::new("http://localhost:30000".to_string())
            .expect("Failed to create ElectrsClient");
        let store = Arc::new(MokaStrore::new());

        let swap_manager =
            SwapManager::new(electrum_client, &cosigner_pubkey, store, Network::Testnet)
                .expect("Failed to create SwapManager");

        // Test with invalid Alice key
        let result = swap_manager
            .new_swap_accounts("invalid_key", &bob_pubkey)
            .await;
        assert!(result.is_err());
        assert!(
            result
                .err()
                .unwrap()
                .to_string()
                .contains("Invalid Alice public key")
        );

        // Test with invalid Bob key
        let result = swap_manager
            .new_swap_accounts(&alice_pubkey, "invalid_key")
            .await;
        assert!(result.is_err());
        assert!(
            result
                .err()
                .unwrap()
                .to_string()
                .contains("Invalid Bob public key")
        );
    }
}
