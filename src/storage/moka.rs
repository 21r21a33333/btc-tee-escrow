use eyre::Result;
use moka::future::{Cache, CacheBuilder};

use crate::{
    models::{Challenge, Swap},
    storage::Storage,
};

pub struct MokaStrore {
    pub swaps: Cache<String, Swap>,
    pub challenges: Cache<String, Challenge>,
}

impl MokaStrore {
    pub fn new() -> Self {
        Self {
            swaps: Cache::builder().name("swaps").build(),
            challenges: Cache::builder().name("challenges").build(),
        }
    }
}

#[async_trait::async_trait]
impl Storage for MokaStrore {
    async fn store_swap(&self, id: &str, swap: &Swap) -> Result<()> {
        self.swaps.insert(id.to_string(), swap.clone()).await;
        Ok(())
    }
    async fn get_swap(&self, id: &str) -> Result<Option<Swap>> {
        Ok(self.swaps.get(id).await)
    }
    async fn store_challenge(&self, id: &str, challenge: Challenge) -> Result<()> {
        self.challenges
            .insert(id.to_string(), challenge.clone())
            .await;
        Ok(())
    }
    async fn get_challenge(&self, id: &str) -> Result<Option<Challenge>> {
        Ok(self.challenges.get(id).await)
    }
}
