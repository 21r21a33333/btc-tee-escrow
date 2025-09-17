use crate::server::{
    bad_request_response, internal_server_error_response, not_found_response, ok_response,
};
use eyre::Result;
use http::{Request, Response};
use serde::{Deserialize, Serialize};

/// Request body for creating a new swap
#[derive(Debug, Deserialize)]
pub struct CreateSwapRequest {
    pub alice_pubkey: String,
    pub bob_pubkey: String,
}

/// Response for swap creation
#[derive(Debug, Serialize)]
pub struct CreateSwapResponse {
    pub success: bool,
    pub swap_id: String,
    pub alice_pubkey: String,
    pub bob_pubkey: String,
    pub alice_wallet_addr: String,
    pub bob_wallet_addr: String,
}

/// Response for swap retrieval
#[derive(Debug, Serialize)]
pub struct GetSwapResponse {
    pub success: bool,
    pub swap_id: String,
    pub alice_pubkey: String,
    pub bob_pubkey: String,
    pub alice_wallet_addr: String,
    pub bob_wallet_addr: String,
    pub challenges: Vec<serde_json::Value>,
}

/// Error response
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub success: bool,
    pub error: String,
}

/// Create a new swap
pub fn create_swap(
    state: &crate::server::AppState,
    request: &Request<String>,
) -> Result<Response<String>> {
    if request.method() != "POST" {
        return Ok(bad_request_response(serde_json::to_string(
            &ErrorResponse {
                success: false,
                error: "Method not allowed. Use POST.".to_string(),
            },
        )?));
    }

    let create_request: CreateSwapRequest = serde_json::from_str(request.body())
        .map_err(|e| eyre::eyre!("Invalid request body: {}", e))?;

    // Validate inputs
    if create_request.alice_pubkey.trim().is_empty() || create_request.bob_pubkey.trim().is_empty()
    {
        return Ok(bad_request_response(serde_json::to_string(
            &ErrorResponse {
                success: false,
                error: "Alice and Bob public keys cannot be empty".to_string(),
            },
        )?));
    }

    // Use tokio runtime to handle async operations
    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| eyre::eyre!("Failed to create Tokio runtime: {}", e))?;
    let swap_manager = state.swap_manager.clone();

    match rt.block_on(async {
        swap_manager
            .new_swap_accounts(&create_request.alice_pubkey, &create_request.bob_pubkey)
            .await
    }) {
        Ok(swap) => {
            let response = CreateSwapResponse {
                success: true,
                swap_id: swap.swap_id.clone(),
                alice_pubkey: swap.alice_pubkey.clone(),
                bob_pubkey: swap.bob_pubkey.clone(),
                alice_wallet_addr: swap.alice_wallet_addr.clone(),
                bob_wallet_addr: swap.bob_wallet_addr.clone(),
            };

            Ok(ok_response(serde_json::to_string(&response)?))
        }
        Err(e) => Ok(bad_request_response(serde_json::to_string(
            &ErrorResponse {
                success: false,
                error: format!("Failed to create swap: {}", e),
            },
        )?)),
    }
}

/// Get a swap by ID
pub fn get_swap(
    state: &crate::server::AppState,
    request: &Request<String>,
) -> Result<Response<String>> {
    if request.method() != "GET" {
        return Ok(bad_request_response(serde_json::to_string(
            &ErrorResponse {
                success: false,
                error: "Method not allowed. Use GET.".to_string(),
            },
        )?));
    }

    // Extract swap_id from path (assuming format: /swaps/{swap_id})
    let path_parts: Vec<&str> = request.uri().path().split('/').collect();
    if path_parts.len() < 3 || path_parts[1] != "swaps" {
        return Ok(bad_request_response(serde_json::to_string(
            &ErrorResponse {
                success: false,
                error: "Invalid path format. Expected /swaps/{swap_id}".to_string(),
            },
        )?));
    }

    let swap_id = path_parts[2];
    if swap_id.trim().is_empty() {
        return Ok(bad_request_response(serde_json::to_string(
            &ErrorResponse {
                success: false,
                error: "Swap ID cannot be empty".to_string(),
            },
        )?));
    }

    // Use tokio runtime to handle async operations
    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| eyre::eyre!("Failed to create Tokio runtime: {}", e))?;
    let swap_manager = state.swap_manager.clone();

    match rt.block_on(async { swap_manager.get_swap(swap_id).await }) {
        Ok(Some(swap)) => {
            let response = GetSwapResponse {
                success: true,
                swap_id: swap.swap_id.clone(),
                alice_pubkey: swap.alice_pubkey.clone(),
                bob_pubkey: swap.bob_pubkey.clone(),
                alice_wallet_addr: swap.alice_wallet_addr.clone(),
                bob_wallet_addr: swap.bob_wallet_addr.clone(),
                challenges: swap.challenges.clone(),
            };

            Ok(ok_response(serde_json::to_string(&response)?))
        }
        Ok(None) => Ok(not_found_response(serde_json::to_string(&ErrorResponse {
            success: false,
            error: format!("Swap not found with ID: {}", swap_id),
        })?)),
        Err(e) => Ok(internal_server_error_response(serde_json::to_string(
            &ErrorResponse {
                success: false,
                error: format!("Failed to retrieve swap: {}", e),
            },
        )?)),
    }
}
