pub mod routers;
pub mod server;

use crate::{
    btc::ElectrsClient,
    services::validator::Validator,
    services::{game::GameManager, swap::SwapManager},
    storage::Storage,
};
use bitcoin::Network;
use eyre::Result;
use http::{Method, Request, Response, StatusCode, Uri};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    str::FromStr,
    sync::Arc,
    thread,
    time::Duration,
};

/// Application state containing managers for swaps and games
#[derive(Clone)]
pub struct AppState {
    pub swap_manager: Arc<SwapManager>,
    pub game_manager: Arc<GameManager>,
}

impl AppState {
    pub fn new(
        electrum_client: ElectrsClient,
        cosigner_pubkey: &str,
        store: Arc<dyn Storage + Send + Sync>,
        validator: Arc<dyn Validator + Send + Sync>,
        network: Network,
    ) -> Result<Self> {
        let swap_manager = Arc::new(SwapManager::new(
            electrum_client,
            cosigner_pubkey,
            store.clone(),
            network,
        )?);

        let game_manager = Arc::new(GameManager::new(store, validator));

        Ok(Self {
            swap_manager,
            game_manager,
        })
    }
}

/// Helper functions for creating HTTP responses
pub fn create_response(status: StatusCode, body: String) -> Response<String> {
    Response::builder()
        .status(status)
        .header("Content-Type", "application/json")
        .header("Content-Length", body.len())
        .body(body)
        .unwrap()
}

pub fn ok_response(body: String) -> Response<String> {
    create_response(StatusCode::OK, body)
}

pub fn bad_request_response(body: String) -> Response<String> {
    create_response(StatusCode::BAD_REQUEST, body)
}

pub fn not_found_response(body: String) -> Response<String> {
    create_response(StatusCode::NOT_FOUND, body)
}

pub fn internal_server_error_response(body: String) -> Response<String> {
    create_response(StatusCode::INTERNAL_SERVER_ERROR, body)
}

/// Convert HTTP Response to bytes for sending over TCP
pub fn response_to_bytes(response: &Response<String>) -> Vec<u8> {
    let status_line = format!(
        "HTTP/1.1 {} {}",
        response.status().as_u16(),
        response.status().canonical_reason().unwrap_or("Unknown")
    );
    let mut response_bytes = format!("{}\r\n", status_line);

    // Add headers
    for (key, value) in response.headers() {
        response_bytes.push_str(&format!("{}: {}\r\n", key, value.to_str().unwrap_or("")));
    }

    // End headers
    response_bytes.push_str("\r\n");

    // Add body
    response_bytes.push_str(response.body());
    response_bytes.into_bytes()
}

/// Parse HTTP request from stream
pub fn parse_http_request(stream: &mut TcpStream) -> Result<Request<String>> {
    let mut buffer = [0; 8192];
    let bytes_read = stream.read(&mut buffer)?;

    if bytes_read == 0 {
        return Err(eyre::eyre!("Empty request"));
    }

    let request_str = String::from_utf8_lossy(&buffer[..bytes_read]);
    let lines: Vec<&str> = request_str.split("\r\n").collect();

    if lines.is_empty() {
        return Err(eyre::eyre!("Invalid request format"));
    }

    let request_line = lines[0];
    let parts: Vec<&str> = request_line.split_whitespace().collect();

    if parts.len() < 3 {
        return Err(eyre::eyre!("Invalid request line"));
    }

    let method = Method::from_str(parts[0])?;
    let uri = Uri::from_str(parts[1])?;
    let _version = parts[2]; // HTTP/1.1

    let mut headers = http::HeaderMap::new();
    let mut body_start = 1;

    for (i, line) in lines.iter().enumerate().skip(1) {
        if line.is_empty() {
            body_start = i + 1;
            break;
        }
        if let Some((key, value)) = line.split_once(':') {
            if let (Ok(header_name), Ok(header_value)) = (
                http::HeaderName::try_from(key.trim()),
                http::HeaderValue::try_from(value.trim()),
            ) {
                headers.insert(header_name, header_value);
            }
        }
    }

    let body = if body_start < lines.len() {
        lines[body_start..].join("\r\n")
    } else {
        String::new()
    };

    let mut request = Request::builder().method(method).uri(uri).body(body)?;

    // Copy headers from parsed headers to the request
    *request.headers_mut() = headers;

    Ok(request)
}

/// Route handler function type
pub type RouteHandler = fn(&AppState, &Request<String>) -> Result<Response<String>>;

/// Router for handling different routes
pub struct Router {
    routes: std::collections::HashMap<String, RouteHandler>,
}

impl Router {
    pub fn new() -> Self {
        Self {
            routes: std::collections::HashMap::new(),
        }
    }

    pub fn add_route(&mut self, path: &str, handler: RouteHandler) {
        self.routes.insert(path.to_string(), handler);
    }

    pub fn handle_request(&self, state: &AppState, request: &Request<String>) -> Response<String> {
        let method = request.method();
        let path = request.uri().path();
        let route_key = format!("{} {}", method, path);

        // Try exact match first
        if let Some(handler) = self.routes.get(&route_key) {
            match handler(state, request) {
                Ok(response) => response,
                Err(e) => internal_server_error_response(format!(r#"{{"error": "{}"}}"#, e)),
            }
        } else {
            // Try pattern matching for dynamic routes
            for (pattern, handler) in &self.routes {
                if self.matches_pattern(pattern, &route_key) {
                    match handler(state, request) {
                        Ok(response) => return response,
                        Err(e) => {
                            return internal_server_error_response(format!(
                                r#"{{"error": "{}"}}"#,
                                e
                            ));
                        }
                    }
                }
            }

            not_found_response(r#"{"error": "Route not found"}"#.to_string())
        }
    }

    fn matches_pattern(&self, pattern: &str, route_key: &str) -> bool {
        // Split pattern into method and path
        let pattern_parts: Vec<&str> = pattern.splitn(2, ' ').collect();
        if pattern_parts.len() != 2 {
            return false;
        }
        let (pattern_method, pattern_path) = (pattern_parts[0], pattern_parts[1]);

        // Split route_key into method and path
        let route_parts: Vec<&str> = route_key.splitn(2, ' ').collect();
        if route_parts.len() != 2 {
            return false;
        }
        let (route_method, route_path) = (route_parts[0], route_parts[1]);

        // Check method match
        if pattern_method != route_method {
            return false;
        }

        // Check path match with parameter support
        if pattern_path.contains(':') {
            let pattern_path_parts: Vec<&str> = pattern_path.split('/').collect();
            let route_path_parts: Vec<&str> = route_path.split('/').collect();

            if pattern_path_parts.len() != route_path_parts.len() {
                return false;
            }

            for (pattern_part, route_part) in pattern_path_parts.iter().zip(route_path_parts.iter())
            {
                if !pattern_part.starts_with(':') && pattern_part != route_part {
                    return false;
                }
            }
            true
        } else {
            pattern_path == route_path
        }
    }
}

/// Handle individual client connection
fn handle_client(mut stream: TcpStream, state: AppState, router: Router) {
    // Set read timeout
    stream.set_read_timeout(Some(Duration::from_secs(30))).ok();

    loop {
        match parse_http_request(&mut stream) {
            Ok(request) => {
                let response = router.handle_request(&state, &request);
                let response_bytes = response_to_bytes(&response);

                if let Err(e) = stream.write_all(&response_bytes) {
                    eprintln!("Error writing response: {}", e);
                    break;
                }

                if let Err(e) = stream.flush() {
                    eprintln!("Error flushing stream: {}", e);
                    break;
                }

                // For HTTP/1.0 or non-persistent connections, close after first request
                if !request
                    .headers()
                    .get("connection")
                    .map(|v| v.to_str().unwrap_or("").to_lowercase() == "keep-alive")
                    .unwrap_or(false)
                {
                    break;
                }
            }
            Err(e) => {
                eprintln!("Error parsing request: {}", e);
                let error_response =
                    bad_request_response(format!(r#"{{"error": "Invalid request: {}"}}"#, e));
                let _ = stream.write_all(&response_to_bytes(&error_response));
                break;
            }
        }
    }
}

/// Start the server
pub fn start_server(addr: &str, state: AppState, router: Router) -> Result<()> {
    let listener = TcpListener::bind(addr)?;
    println!("Server listening on {}", addr);

    // Create a thread pool for handling concurrent connections
    let _thread_pool_size = num_cpus::get() * 2; // Adjust as needed

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let state_clone = state.clone();
                let router_clone = router.clone();

                // For simplicity, spawn a new thread for each connection
                // In production, you'd want to use a proper thread pool
                thread::spawn(move || {
                    handle_client(stream, state_clone, router_clone);
                });
            }
            Err(e) => {
                eprintln!("Error accepting connection: {}", e);
            }
        }
    }

    Ok(())
}

impl Clone for Router {
    fn clone(&self) -> Self {
        Self {
            routes: self.routes.clone(),
        }
    }
}
