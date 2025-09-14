use crate::client::ElectrsClient;

mod client;
fn main() {
    // Initialize the client with the provided URL
    let client = ElectrsClient::new("http://167.235.89.144:3000".to_string())
        .expect("Failed to create ElectrsClient");

    // Transaction ID to fetch
    let txid = "9b719837e651ae3579168287a3aa04c4b7a3613ce2256f4ac25bbc9e9f9a21b1";

    // Fetch transaction details
    match client.get_transaction(txid) {
        Ok(tx) => {
            println!("Transaction fetched successfully!");
            println!("Transaction ID: {}", txid);
            println!("Inputs: {}", tx.input.len());
            println!("Outputs: {}", tx.output.len());
            println!("Version: {}", tx.version);
            println!("Locktime: {}", tx.lock_time);
        }
        Err(e) => {
            eprintln!("Failed to fetch transaction: {}", e);
            panic!("Test failed: {}", e);
        }
    }
}
