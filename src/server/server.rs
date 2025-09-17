use crate::{
    btc::ElectrsClient,
    server::routers::{game, swap},
    server::{AppState, Router, start_server},
    services::validator::Validator,
    storage::{MokaStrore, Storage},
};
use bitcoin::Network;
use eyre::Result;
use std::sync::Arc;

/// Initialize and start the TEE-compatible server
pub fn start_tee_server(
    listen_addr: &str,
    electrum_url: &str,
    cosigner_pubkey: &str,
    network: Network,
) -> Result<()> {
    println!("Starting TEE-compatible BTC Escrow Server...");
    println!("Listen address: {}", listen_addr);
    println!("Electrum URL: {}", electrum_url);
    println!("Cosigner pubkey: {}", cosigner_pubkey);
    println!("Network: {:?}", network);

    // Initialize storage
    let store: Arc<dyn Storage + Send + Sync> = Arc::new(MokaStrore::new());

    // Initialize electrum client
    let electrum_client = ElectrsClient::new(electrum_url.to_string())
        .map_err(|e| eyre::eyre!("Failed to create electrum client: {}", e))?;

    // Initialize validator (you'll need to implement this based on your validator service)
    let validator: Arc<dyn Validator + Send + Sync> =
        Arc::new(crate::services::validator::BtcValidator::new());

    // Initialize application state
    let app_state = AppState::new(electrum_client, cosigner_pubkey, store, validator, network)?;

    // Initialize router and register routes
    let mut router = Router::new();

    // Swap routes
    router.add_route("POST /swaps", swap::create_swap);
    router.add_route("GET /swaps/:id", swap::get_swap);

    // Game routes
    router.add_route("POST /games", game::init_game);
    router.add_route("POST /games/play", game::play_game);
    router.add_route("GET /games/:id", game::get_game_state);
    router.add_route("GET /games/:id/ready", game::check_game_ready);

    // Health check route
    router.add_route("GET /health", health_check);

    // Start the server
    start_server(listen_addr, app_state, router)
}

/// Health check endpoint
fn health_check(
    _state: &AppState,
    _request: &http::Request<String>,
) -> Result<http::Response<String>> {
    let response = serde_json::json!({
        "status": "healthy",
        "service": "btc-escrow-tee",
        "timestamp": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    });

    Ok(crate::server::ok_response(serde_json::to_string(
        &response,
    )?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::MokaStrore;
    use std::sync::Arc;

    #[test]
    fn test_health_check() {
        let store: Arc<dyn Storage + Send + Sync> = Arc::new(MokaStrore::new());
        let electrum_client = ElectrsClient::new("http://localhost:30000".to_string()).unwrap();
        let validator: Arc<dyn Validator + Send + Sync> =
            Arc::new(crate::services::validator::BtcValidator::new());

        let app_state = AppState::new(
            electrum_client,
            "test_cosigner_pubkey",
            store,
            validator,
            Network::Testnet,
        )
        .unwrap();

        let request = http::Request::builder()
            .method("GET")
            .uri("/health")
            .body(String::new())
            .unwrap();

        let response = health_check(&app_state, &request).unwrap();
        assert_eq!(response.status().as_u16(), 200);
        assert!(response.body().contains("healthy"));
    }
}
