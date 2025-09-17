use crate::models::{Challenge, Swap};
use eyre::Result;
mod moka;

pub use moka::*;

#[async_trait::async_trait]
pub trait Storage {
    async fn store_swap(&self, id: &str, swap: &Swap) -> Result<()>;
    async fn get_swap(&self, id: &str) -> Result<Option<Swap>>;
    async fn store_challenge(&self, id: &str, challenge: Challenge) -> Result<()>;
    async fn get_challenge(&self, id: &str) -> Result<Option<Challenge>>;
    async fn clear_challenge(&self, id: &str) -> Result<()>;
    async fn append_challenge(&self, swap_id: &str, challenge: Challenge) -> Result<()>;
}
