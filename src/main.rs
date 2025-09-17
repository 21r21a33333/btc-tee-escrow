use ::bitcoin::Network;
mod btc;
mod models;
mod server;
mod services;
mod storage;

// network ideally should be in config
pub const NETWORK: Network = Network::Regtest;

fn main() -> eyre::Result<()> {
    // Example configuration - in production, these should come from environment variables or config files
    let listen_addr = "127.0.0.1:8080";
    let electrum_url = "http://localhost:30000"; // Electrum server URL
    let cosigner_pubkey = "your_cosigner_public_key_here"; // Replace with actual cosigner public key

    println!("Starting BTC Escrow TEE Server...");
    println!("Network: {:?}", NETWORK);

    // Start the server
    server::server::start_tee_server(listen_addr, electrum_url, cosigner_pubkey, NETWORK)?;

    Ok(())
}
