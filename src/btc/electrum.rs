use bitcoin::Transaction;
use http::{Request, StatusCode};
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::io::{Read, Write};
use std::net::TcpStream;

pub const BITCOIN_DUST_AMOUNT: u64 = 546; // in satoshis

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UTXO {
    pub status: Status,
    pub txid: String,
    pub value: u64,
    pub vout: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Status {
    pub block_hash: Option<String>,
    pub block_height: Option<u64>,
    pub block_time: Option<u64>,
    pub confirmed: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BlockStatus {
    pub confirmed: bool,
    pub block_height: Option<u32>,
    pub block_hash: Option<String>,
    pub block_time: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TransactionStatus {
    pub confirmed: bool,
    pub block_height: Option<u32>,
    pub block_hash: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OutspendStatus {
    pub spent: bool,
    pub txid: Option<String>,
    pub vin: Option<u32>,
    pub status: Option<BlockStatus>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AddressStats {
    pub tx_count: u32,
    pub funded_txo_count: u32,
    pub funded_txo_sum: u64,
    pub spent_txo_count: u32,
    pub spent_txo_sum: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AddressInfo {
    pub address: String,
    pub chain_stats: AddressStats,
    pub mempool_stats: AddressStats,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Prevout {
    pub scriptpubkey: String,
    pub scriptpubkey_asm: String,
    pub scriptpubkey_type: String,
    pub scriptpubkey_address: String,
    pub value: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Vin {
    pub txid: String,
    pub vout: u32,
    pub prevout: Prevout,
    pub scriptsig: String,
    pub scriptsig_asm: String,
    pub witness: Vec<String>,
    pub is_coinbase: bool,
    pub sequence: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Vout {
    pub scriptpubkey: String,
    pub scriptpubkey_asm: String,
    pub scriptpubkey_type: String,
    pub scriptpubkey_address: String,
    pub value: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TransactionVerbose {
    pub txid: String,
    pub version: u32,
    pub locktime: u32,
    pub vin: Vec<Vin>,
    pub vout: Vec<Vout>,
    pub size: u32,
    pub weight: u32,
    pub fee: u64,
    pub status: Status,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TransactionWithFee {
    pub tx: Transaction,
    pub fee: u64,
}

#[derive(Clone, Debug)]
pub struct ElectrsClient {
    base_url: String,
    host: String,
    port: u16,
}

impl ElectrsClient {
    pub fn new(base_url: String) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let url = url::Url::parse(&base_url)?;
        let host = url.host_str().ok_or("Invalid host in URL")?.to_string();
        let port = url.port().unwrap_or(80);
        Ok(ElectrsClient {
            base_url,
            host,
            port,
        })
    }

    pub fn send_request(
        &self,
        request: Request<Vec<u8>>,
    ) -> Result<(StatusCode, Vec<u8>), Box<dyn Error + Send + Sync>> {
        // Connect to the server
        let addr = format!("{}:{}", self.host, self.port);
        let mut stream = TcpStream::connect(&addr)
            .map_err(|e| format!("Failed to connect to {}: {}", addr, e))?;

        let host_header = if self.port == 80 || self.port == 443 {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        };

        // Build HTTP request with proper headers
        let mut request_bytes = format!(
            "{} {} HTTP/1.1\r\nHost: {}\r\nContent-Length: {}\r\n",
            request.method(),
            request.uri(),
            host_header,
            request.body().len()
        );

        // Add other headers from the request
        for (name, value) in request.headers() {
            if name != "host" && name != "content-length" {
                request_bytes.push_str(&format!("{}: {}\r\n", name, value.to_str().unwrap_or("")));
            }
        }

        // Add Connection: close to ensure the server closes the connection after response
        request_bytes.push_str("Connection: close\r\n");

        // End headers
        request_bytes.push_str("\r\n");

        // Send request
        stream
            .write_all(request_bytes.as_bytes())
            .map_err(|e| format!("Failed to send request headers: {}", e))?;

        if !request.body().is_empty() {
            stream
                .write_all(request.body())
                .map_err(|e| format!("Failed to send request body: {}", e))?;
        }

        stream
            .flush()
            .map_err(|e| format!("Failed to flush stream: {}", e))?;

        println!("DEBUG: Request sent, reading response...");

        // Set a read timeout to avoid hanging
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(30)))
            .map_err(|e| format!("Failed to set read timeout: {}", e))?;

        // Read response
        let mut response = Vec::new();
        match stream.read_to_end(&mut response) {
            Err(e) => {
                return Err(format!("Failed to read response: {}", e).into());
            }
            Ok(_) => (),
        }

        if response.is_empty() {
            return Err("Empty response from server".into());
        }

        // Parse response
        let response_str = String::from_utf8_lossy(&response);

        // Find the end of headers - try both \r\n\r\n and \n\n
        let header_end = response_str
            .find("\r\n\r\n")
            .or_else(|| response_str.find("\n\n"))
            .ok_or("Invalid HTTP response: no header separator found")?;

        let headers_part = &response_str[..header_end];
        let body_start = if response_str[header_end..].starts_with("\r\n\r\n") {
            header_end + 4
        } else {
            header_end + 2
        };

        // Split headers by line (handle both \r\n and \n)
        let header_lines: Vec<&str> = if headers_part.contains("\r\n") {
            headers_part.split("\r\n").collect()
        } else {
            headers_part.split('\n').collect()
        };

        let status_line = header_lines.get(0).ok_or("Missing status line")?;

        let status_parts: Vec<&str> = status_line.split_whitespace().collect();

        if status_parts.len() < 2 {
            return Err(format!("Invalid status line: {}", status_line).into());
        }

        let status_code = status_parts[1]
            .parse::<u16>()
            .map_err(|e| format!("Invalid status code '{}': {}", status_parts[1], e))?;

        let status = StatusCode::from_u16(status_code)
            .map_err(|e| format!("Invalid status code {}: {}", status_code, e))?;

        // Extract body as bytes (not as string to avoid UTF-8 issues)
        let body = if body_start < response.len() {
            response[body_start..].to_vec()
        } else {
            Vec::new()
        };

        Ok((status, body))
    }

    // Transaction APIs
    pub fn get_transaction(&self, txid: &str) -> Result<Transaction, Box<dyn Error + Send + Sync>> {
        let url = format!("{}/tx/{}/raw", self.base_url, txid);
        let request = Request::builder()
            .method("GET")
            .uri(&url)
            .body(Vec::new())?;
        let (status, body) = self.send_request(request)?;
        if !status.is_success() {
            return Err(format!("Server returned error {}: {}", status, url).into());
        }
        Ok(bitcoin::consensus::deserialize(&body)?)
    }

    pub fn get_transaction_status(
        &self,
        txid: &str,
    ) -> Result<TransactionStatus, Box<dyn Error + Send + Sync>> {
        let url = format!("{}/tx/{}/status", self.base_url, txid);
        let request = Request::builder()
            .method("GET")
            .uri(&url)
            .body(Vec::new())?;
        let (status, body) = self.send_request(request)?;
        if !status.is_success() {
            return Err(format!("Server returned error {}: {}", status, url).into());
        }
        Ok(serde_json::from_slice(&body)?)
    }

    pub fn get_transaction_hex(&self, txid: &str) -> Result<String, Box<dyn Error + Send + Sync>> {
        let url = format!("{}/tx/{}/hex", self.base_url, txid);
        let request = Request::builder()
            .method("GET")
            .uri(&url)
            .body(Vec::new())?;
        let (status, body) = self.send_request(request)?;
        if !status.is_success() {
            return Err(format!("Server returned error {}: {}", status, url).into());
        }
        Ok(String::from_utf8(body)?)
    }

    pub fn get_transaction_outspend(
        &self,
        txid: &str,
        vout: u32,
    ) -> Result<OutspendStatus, Box<dyn Error + Send + Sync>> {
        let url = format!("{}/tx/{}/outspend/{}", self.base_url, txid, vout);
        let request = Request::builder()
            .method("GET")
            .uri(&url)
            .body(Vec::new())?;
        let (status, body) = self.send_request(request)?;
        if !status.is_success() {
            return Err(format!("Server returned error {}: {}", status, url).into());
        }
        Ok(serde_json::from_slice(&body)?)
    }

    pub fn get_transaction_outspends(
        &self,
        txid: &str,
    ) -> Result<Vec<OutspendStatus>, Box<dyn Error + Send + Sync>> {
        let url = format!("{}/tx/{}/outspends", self.base_url, txid);
        let request = Request::builder()
            .method("GET")
            .uri(&url)
            .body(Vec::new())?;
        let (status, body) = self.send_request(request)?;
        if !status.is_success() {
            return Err(format!("Server returned error {}: {}", status, url).into());
        }
        Ok(serde_json::from_slice(&body)?)
    }

    // Address APIs
    pub fn get_address_info(
        &self,
        address: &str,
    ) -> Result<AddressInfo, Box<dyn Error + Send + Sync>> {
        let url = format!("{}/address/{}", self.base_url, address);
        let request = Request::builder()
            .method("GET")
            .uri(&url)
            .body(Vec::new())?;
        let (status, body) = self.send_request(request)?;
        if !status.is_success() {
            return Err(format!("Server returned error {}: {}", status, url).into());
        }
        Ok(serde_json::from_slice(&body)?)
    }

    pub fn get_address_transactions(
        &self,
        address: &str,
    ) -> Result<Vec<Transaction>, Box<dyn Error + Send + Sync>> {
        let url = format!("{}/address/{}/txs", self.base_url, address);
        let request = Request::builder()
            .method("GET")
            .uri(&url)
            .body(Vec::new())?;
        let (status, body) = self.send_request(request)?;
        if !status.is_success() {
            return Err(format!("Server returned error {}: {}", status, url).into());
        }
        let txs: Vec<Vec<u8>> = serde_json::from_slice(&body)?;
        let mut transactions = Vec::new();
        for tx_bytes in txs {
            transactions.push(bitcoin::consensus::deserialize(&tx_bytes)?);
        }
        Ok(transactions)
    }

    pub fn get_pending_transactions(
        &self,
        address: &str,
    ) -> Result<Vec<TransactionWithFee>, Box<dyn Error + Send + Sync>> {
        let url = format!("{}/address/{}/txs/mempool", self.base_url, address);
        let request = Request::builder()
            .method("GET")
            .uri(&url)
            .body(Vec::new())?;
        let (status, body) = self.send_request(request)?;
        if !status.is_success() {
            return Err(format!("Server returned error {}: {}", status, url).into());
        }
        let txs: Vec<TransactionVerbose> = serde_json::from_slice(&body)?;
        let mut transactions = vec![];
        for tx in txs {
            transactions.push(TransactionWithFee {
                tx: self.get_transaction(&tx.txid)?,
                fee: tx.fee,
            });
        }
        Ok(transactions)
    }

    pub fn get_address_utxos(
        &self,
        address: &str,
    ) -> Result<Vec<UTXO>, Box<dyn Error + Send + Sync>> {
        let url = format!("{}/address/{}/utxo", self.base_url, address);
        let request = Request::builder()
            .method("GET")
            .uri(&url)
            .body(Vec::new())?;
        let (status, body) = self.send_request(request)?;
        if !status.is_success() {
            return Err(format!("Server returned error {}: {}", status, url).into());
        }
        Ok(serde_json::from_slice(&body)?)
    }

    // Block APIs
    pub fn get_block_height(&self) -> Result<u32, Box<dyn Error + Send + Sync>> {
        let url = format!("{}/blocks/tip/height", self.base_url);
        let request = Request::builder()
            .method("GET")
            .uri(&url)
            .body(Vec::new())?;
        let (status, body) = self.send_request(request)?;
        if !status.is_success() {
            return Err(format!("Server returned error {}: {}", status, url).into());
        }
        Ok(String::from_utf8(body)?.parse()?)
    }

    pub fn get_block_hash(&self) -> Result<String, Box<dyn Error + Send + Sync>> {
        let url = format!("{}/blocks/tip/hash", self.base_url);
        let request = Request::builder()
            .method("GET")
            .uri(&url)
            .body(Vec::new())?;
        let (status, body) = self.send_request(request)?;
        if !status.is_success() {
            return Err(format!("Server returned error {}: {}", status, url).into());
        }
        Ok(String::from_utf8(body)?)
    }

    pub fn get_block_txids(
        &self,
        block_hash: &str,
    ) -> Result<Vec<String>, Box<dyn Error + Send + Sync>> {
        let url = format!("{}/block/{}/txids", self.base_url, block_hash);
        let request = Request::builder()
            .method("GET")
            .uri(&url)
            .body(Vec::new())?;
        let (status, body) = self.send_request(request)?;
        if !status.is_success() {
            return Err(format!("Server returned error {}: {}", status, url).into());
        }
        Ok(serde_json::from_slice(&body)?)
    }

    // Mempool APIs
    pub fn get_mempool_txids(&self) -> Result<Vec<String>, Box<dyn Error + Send + Sync>> {
        let url = format!("{}/mempool/txids", self.base_url);
        let request = Request::builder()
            .method("GET")
            .uri(&url)
            .body(Vec::new())?;
        let (status, body) = self.send_request(request)?;
        if !status.is_success() {
            return Err(format!("Server returned error {}: {}", status, url).into());
        }
        Ok(serde_json::from_slice(&body)?)
    }

    pub fn broadcast_transaction(
        &self,
        tx: &Transaction,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let url = format!("{}/tx", self.base_url);
        let tx_hex = hex::encode(bitcoin::consensus::serialize(tx));
        let request = Request::builder()
            .method("POST")
            .uri(&url)
            .body(tx_hex.into_bytes())?;
        let (status, body) = self.send_request(request)?;
        if !status.is_success() {
            return Err(format!(
                "Server returned error {}: {}. Response: {}",
                status,
                url,
                String::from_utf8_lossy(&body)
            )
            .into());
        }
        Ok(String::from_utf8(body)?)
    }

    pub fn get_fee_estimates(
        &self,
    ) -> Result<std::collections::HashMap<String, f64>, Box<dyn Error + Send + Sync>> {
        let url = format!("{}/fee-estimates", self.base_url);
        let request = Request::builder()
            .method("GET")
            .uri(&url)
            .body(Vec::new())?;
        let (status, body) = self.send_request(request)?;
        if !status.is_success() {
            return Err(format!("Server returned error {}: {}", status, url).into());
        }
        Ok(serde_json::from_slice(&body)?)
    }
}

pub struct Merry {
    client: ElectrsClient,
}

impl Merry {
    pub fn new() -> Self {
        Merry {
            client: ElectrsClient::new("http://localhost:30000".to_string()).unwrap(),
        }
    }

    pub fn fund(&self, address: &str) {
        std::process::Command::new("merry")
            .arg("faucet")
            .arg("--to")
            .arg(&address)
            .output()
            .expect("Failed to execute merry faucet command");
    }

    pub fn mine(&self) {
        std::process::Command::new("merry")
            .arg("rpc")
            .arg("generatetoaddress")
            .arg("1")
            .arg("bcrt1qcw3q79t2zwd8h5mjnvwh8uv6k3zz8g7kk9t5e8")
            .output()
            .expect("Failed to execute merry mine command");
    }

    pub fn client(&self) -> &ElectrsClient {
        &self.client
    }
}

#[cfg(test)]
mod test {
    use crate::btc::electrum::ElectrsClient;

    #[tokio::test]
    async fn test_get_transaction() {
        // Initialize the client with the provided URL
        let client = ElectrsClient::new("http://127.0.0.1:30000".to_string())
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
}
