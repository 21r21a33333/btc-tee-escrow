use crate::{
    models::{Challenge, GameState, Swap},
    services::validator::Validator,
    storage::Storage,
};
use eyre::{Result, eyre};
use std::{ops::Deref, sync::Arc};

/// Manages coin flip games associated with swaps
pub struct GameManager {
    store: Arc<dyn Storage + Send + Sync>,
    validator: Arc<dyn Validator + Send + Sync>,
}

impl GameManager {
    /// Creates a new GameManager instance
    pub fn new(
        store: Arc<dyn Storage + Send + Sync>,
        validator: Arc<dyn Validator + Send + Sync>,
    ) -> Self {
        Self { store, validator }
    }

    /// Initializes a new game for a swap
    ///
    /// Validates the SACp and swap ID, checks for existing games, and stores the initial game state.
    pub async fn init(&self, sacp: &str, swap_id: &str) -> Result<()> {
        self.validate_inputs(sacp, swap_id)?;
        if self.store.get_challenge(swap_id).await?.is_some() {
            return Err(eyre!("Game already exists for swap_id: {}", swap_id));
        }
        let swap = self.get_swap(swap_id).await?;
        self.validator
            .validate(sacp, swap)
            .await
            .map_err(|e| eyre!("Invalid SACp: {}", e))?;
        let challenge = Challenge::new(
            swap_id.to_string(),
            sacp.to_string(),
            String::new(),
            GameState::None,
        );
        self.store.store_challenge(swap_id, challenge).await
    }

    /// Plays the coin flip game
    ///
    /// Validates Bob's SACp, determines the outcome, updates the challenge, and returns the winning SACp and state.
    pub async fn play(&self, sacp: &str, swap_id: &str) -> Result<(String, GameState)> {
        self.validate_inputs(sacp, swap_id)?;
        let mut challenge = self
            .store
            .get_challenge(swap_id)
            .await?
            .ok_or_else(|| eyre!("No game found for swap_id: {}", swap_id))?;
        let swap = self.get_swap(swap_id).await?;
        self.validator
            .validate(sacp, swap)
            .await
            .map_err(|e| eyre!("Invalid SACp: {}", e))?;
        challenge.bob_partial_tx_hex = sacp.to_string();
        let game_state = self.determine_game_outcome();
        challenge.game_state = game_state.clone();

        let ret_sacp = match game_state {
            GameState::Heads => challenge.alice_partial_tx_hex.deref().to_string(),
            GameState::Tails => challenge.bob_partial_tx_hex.deref().to_string(),
            GameState::None => String::new(),
        };
        self.store.append_challenge(swap_id, challenge).await?;
        self.store.clear_challenge(swap_id).await?;
        Ok((ret_sacp, game_state))
    }

    /// Retrieves the current game state for a swap
    pub async fn get_game_state(&self, swap_id: &str) -> Result<Option<Challenge>> {
        if swap_id.trim().is_empty() {
            return Err(eyre!("Empty swap_id provided"));
        }
        self.store.get_challenge(swap_id).await
    }

    /// Checks if a game is ready to play (Alice's SACp submitted, Bob's not)
    pub async fn is_ready(&self, swap_id: &str) -> Result<bool> {
        if swap_id.trim().is_empty() {
            return Err(eyre!("Empty swap_id provided"));
        }
        Ok(self
            .store
            .get_challenge(swap_id)
            .await?
            .map(|c| !c.alice_partial_tx_hex.is_empty() && c.bob_partial_tx_hex.is_empty())
            .unwrap_or(false))
    }

    /// Validates input SACp and swap_id
    fn validate_inputs(&self, sacp: &str, swap_id: &str) -> Result<()> {
        if sacp.trim().is_empty() || swap_id.trim().is_empty() {
            return Err(eyre!("Empty SACp or swap_id provided"));
        }
        Ok(())
    }

    /// Retrieves a swap, returning an error if not found
    async fn get_swap(&self, swap_id: &str) -> Result<Swap> {
        self.store
            .get_swap(swap_id)
            .await?
            .ok_or_else(|| eyre!("Swap not found for swap_id: {}", swap_id))
    }

    /// Randomly determines the game outcome (heads or tails)
    fn determine_game_outcome(&self) -> GameState {
        if rand::random_bool(0.5) {
            GameState::Heads
        } else {
            GameState::Tails
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        models::{Challenge, GameState, Swap},
        storage::MokaStrore,
    };
    use std::sync::Arc;

    struct MockValidator {}
    #[async_trait::async_trait]
    impl Validator for MockValidator {
        async fn validate(&self, _: &str, _: Swap) -> Result<()> {
            Ok(())
        }
    }

    async fn setup_gm_and_swap(swap_id: &str) -> (GameManager, Arc<MokaStrore>) {
        let store = Arc::new(MokaStrore::new());
        let gm = GameManager::new(store.clone(), Arc::new(MockValidator {}));
        let swap = Swap::new(
            "alice_pubkey".to_string(),
            "bob_pubkey".to_string(),
            "alice_addr".to_string(),
            "bob_addr".to_string(),
        );
        store.store_swap(swap_id, &swap).await.unwrap();
        (gm, store)
    }

    #[tokio::test]
    async fn test_init_success_and_errors() {
        let (gm, store) = setup_gm_and_swap("swap1").await;

        // Success: should create a new challenge
        let res = gm.init("alice_sacp", "swap1").await;
        assert!(res.is_ok());
        let challenge = store.get_challenge("swap1").await.unwrap().unwrap();
        assert_eq!(challenge.swap_id, "swap1");
        assert_eq!(challenge.alice_partial_tx_hex, "alice_sacp");
        assert_eq!(challenge.bob_partial_tx_hex, "");
        assert_eq!(challenge.game_state, GameState::None);

        // Error: empty SACp
        let err = gm.init("", "swap1").await.unwrap_err();
        assert_eq!(err.to_string(), "Empty SACp or swap_id provided");

        // Error: empty swap_id
        let err = gm.init("alice_sacp", "").await.unwrap_err();
        assert_eq!(err.to_string(), "Empty SACp or swap_id provided");

        // Error: game already exists
        let err = gm.init("another_sacp", "swap1").await.unwrap_err();
        assert_eq!(err.to_string(), "Game already exists for swap_id: swap1");

        // Error: swap not found
        let (gm2, _) = setup_gm_and_swap("swap2").await;
        let err = gm2.init("alice_sacp", "swap3").await.unwrap_err();
        assert_eq!(err.to_string(), "Swap not found for swap_id: swap3");
    }

    #[tokio::test]
    async fn test_play_success_and_errors() {
        let (gm, store) = setup_gm_and_swap("swap1").await;
        gm.init("alice_sacp", "swap1").await.unwrap();

        // Test both Heads and Tails outcomes
        let mut saw_heads = false;
        let mut saw_tails = false;
        for _ in 0..20 {
            let (sacp, state) = gm.play("bob_sacp", "swap1").await.unwrap();
            let swap = store.get_swap("swap1").await.unwrap().unwrap();
            let challenge: Challenge =
                serde_json::from_value(swap.challenges.last().unwrap().clone()).unwrap();

            assert_eq!(challenge.bob_partial_tx_hex, "bob_sacp");
            match state {
                GameState::Heads => {
                    assert_eq!(sacp, "alice_sacp");
                    assert_eq!(challenge.game_state, GameState::Heads);
                    saw_heads = true;
                }
                GameState::Tails => {
                    assert_eq!(sacp, "bob_sacp");
                    assert_eq!(challenge.game_state, GameState::Tails);
                    saw_tails = true;
                }
                _ => panic!("Unexpected game state"),
            }
            if saw_heads && saw_tails {
                break;
            }

            // Reset challenge for next iteration
            store
                .store_challenge(
                    "swap1",
                    Challenge {
                        swap_id: "swap1".to_string(),
                        alice_partial_tx_hex: "alice_sacp".to_string(),
                        bob_partial_tx_hex: "".to_string(),
                        game_state: GameState::None,
                    },
                )
                .await
                .unwrap();
        }
        assert!(saw_heads, "Did not observe Heads outcome");
        assert!(saw_tails, "Did not observe Tails outcome");

        // Error: no game found
        let (gm2, _) = setup_gm_and_swap("swap2").await;
        let err = gm2.play("bob_sacp", "swap2").await.unwrap_err();
        assert_eq!(err.to_string(), "No game found for swap_id: swap2");

        // Error: empty SACp
        let err = gm.play("", "swap1").await.unwrap_err();
        assert_eq!(err.to_string(), "Empty SACp or swap_id provided");

        // Error: empty swap_id
        let err = gm.play("bob_sacp", "").await.unwrap_err();
        assert_eq!(err.to_string(), "Empty SACp or swap_id provided");
    }

    #[tokio::test]
    async fn test_get_game_state_and_is_ready() {
        let (gm, store) = setup_gm_and_swap("swap1").await;

        // No challenge yet
        assert!(gm.get_game_state("swap1").await.unwrap().is_none());
        assert!(!gm.is_ready("swap1").await.unwrap());

        // After init
        gm.init("alice_sacp", "swap1").await.unwrap();
        let challenge = gm.get_game_state("swap1").await.unwrap().unwrap();
        assert_eq!(challenge.alice_partial_tx_hex, "alice_sacp");
        assert_eq!(challenge.bob_partial_tx_hex, "");
        assert_eq!(challenge.game_state, GameState::None);
        assert!(gm.is_ready("swap1").await.unwrap());

        // After play
        gm.play("bob_sacp", "swap1").await.unwrap();
        assert!(!gm.is_ready("swap1").await.unwrap());

        // Error: empty swap_id
        let err = gm.get_game_state("").await.unwrap_err();
        assert_eq!(err.to_string(), "Empty swap_id provided");
        let err = gm.is_ready("").await.unwrap_err();
        assert_eq!(err.to_string(), "Empty swap_id provided");
    }
}
