use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// User-facing Swap struct.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Swap {
    pub swap_id: String,
    pub alice_pubkey: String,
    pub bob_pubkey: String,
    pub alice_wallet_addr: String,
    pub bob_wallet_addr: String,
    pub challenges: Vec<Value>, // Vec of game presentation structs as JSON
}

impl Swap {
    pub fn new(
        alice_pubkey: String,
        bob_pubkey: String,
        alice_addr: String,
        bob_addr: String,
    ) -> Self {
        //swapid is sha256(alice_pubkey,bob_pubkey)
        let mut hasher = Sha256::new();
        hasher.update(alice_pubkey.as_bytes());
        hasher.update(bob_pubkey.as_bytes());
        let swap_id = hasher.finalize();
        let swap_id = hex::encode(swap_id);
        Self {
            swap_id,
            alice_pubkey,
            bob_pubkey,
            alice_wallet_addr: alice_addr,
            bob_wallet_addr: bob_addr,
            challenges: Vec::new(),
        }
    }
}

// src/models/challenge.rs
/// Internal Challenge struct for game state.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Challenge {
    pub swap_id: String,
    pub alice_partial_tx_hex: String, // Renamed from sacp; validated partial TX without Bob's sig
    pub bob_partial_tx_hex: String,   // Without Alice's sig
    pub game_state: GameState,        // Random outcome
}

impl Challenge {
    pub fn new(
        swap_id: String,
        alice_partial_tx_hex: String,
        bob_partial_tx_hex: String,
        game_state: GameState,
    ) -> Self {
        Self {
            swap_id,
            alice_partial_tx_hex,
            bob_partial_tx_hex,
            game_state,
        }
    }
}

/// Enum for coin flip outcome.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum GameState {
    Heads, // Alice wins
    Tails, // Bob wins
    None,
}
