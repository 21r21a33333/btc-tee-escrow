use serde::{Deserialize, Serialize};
use serde_json::Value;

/// User-facing Swap struct.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Swap {
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
        Self {
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
#[derive(Clone, Debug)]
pub struct Challenge {
    pub swap_id: String,
    pub alice_partial_tx_hex: String, // Renamed from sacp; validated partial TX without Bob's sig
    pub bob_partial_tx_hex: String,   // Without Alice's sig
    pub game_state: GameState,        // Random outcome
}

/// Enum for coin flip outcome.
#[derive(Clone, Debug, PartialEq)]
pub enum GameState {
    Heads, // Alice wins
    Tails, // Bob wins
}
