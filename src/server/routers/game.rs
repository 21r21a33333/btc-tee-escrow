use crate::{
    models::GameState,
    server::{
        bad_request_response, internal_server_error_response, not_found_response, ok_response,
    },
};
use eyre::Result;
use http::{Request, Response};
use serde::{Deserialize, Serialize};

/// Request body for initializing a game
#[derive(Debug, Deserialize)]
pub struct InitGameRequest {
    pub sacp: String,
    pub swap_id: String,
}

/// Request body for playing a game
#[derive(Debug, Deserialize)]
pub struct PlayGameRequest {
    pub sacp: String,
    pub swap_id: String,
}

/// Response for game initialization
#[derive(Debug, Serialize)]
pub struct InitGameResponse {
    pub success: bool,
    pub swap_id: String,
    pub message: String,
}

/// Response for game play
#[derive(Debug, Serialize)]
pub struct PlayGameResponse {
    pub success: bool,
    pub swap_id: String,
    pub winning_sacp: String,
    pub game_state: GameState,
}

/// Response for game state retrieval
#[derive(Debug, Serialize)]
pub struct GetGameStateResponse {
    pub success: bool,
    pub swap_id: String,
    pub alice_partial_tx_hex: String,
    pub bob_partial_tx_hex: String,
    pub game_state: GameState,
    pub is_ready: bool,
}

/// Response for game readiness check
#[derive(Debug, Serialize)]
pub struct GameReadyResponse {
    pub success: bool,
    pub swap_id: String,
    pub is_ready: bool,
}

/// Error response
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub success: bool,
    pub error: String,
}

/// Initialize a new game
pub fn init_game(
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

    let init_request: InitGameRequest = serde_json::from_str(request.body())
        .map_err(|e| eyre::eyre!("Invalid request body: {}", e))?;

    // Validate inputs
    if init_request.sacp.trim().is_empty() || init_request.swap_id.trim().is_empty() {
        return Ok(bad_request_response(serde_json::to_string(
            &ErrorResponse {
                success: false,
                error: "SACp and swap_id cannot be empty".to_string(),
            },
        )?));
    }

    // Use tokio runtime to handle async operations
    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| eyre::eyre!("Failed to create Tokio runtime: {}", e))?;

    let game_manager = state.game_manager.clone();

    match rt.block_on(async {
        game_manager
            .init(&init_request.sacp, &init_request.swap_id)
            .await
    }) {
        Ok(()) => {
            let response = InitGameResponse {
                success: true,
                swap_id: init_request.swap_id,
                message: "Game initialized successfully".to_string(),
            };

            Ok(ok_response(serde_json::to_string(&response)?))
        }
        Err(e) => Ok(bad_request_response(serde_json::to_string(
            &ErrorResponse {
                success: false,
                error: format!("Failed to initialize game: {}", e),
            },
        )?)),
    }
}

/// Play a game (Bob's turn)
pub fn play_game(
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

    let play_request: PlayGameRequest = serde_json::from_str(request.body())
        .map_err(|e| eyre::eyre!("Invalid request body: {}", e))?;

    // Validate inputs
    if play_request.sacp.trim().is_empty() || play_request.swap_id.trim().is_empty() {
        return Ok(bad_request_response(serde_json::to_string(
            &ErrorResponse {
                success: false,
                error: "SACp and swap_id cannot be empty".to_string(),
            },
        )?));
    }

    // Use tokio runtime to handle async operations
    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| eyre::eyre!("Failed to create Tokio runtime: {}", e))?;
    let game_manager = state.game_manager.clone();

    match rt.block_on(async {
        game_manager
            .play(&play_request.sacp, &play_request.swap_id)
            .await
    }) {
        Ok((winning_sacp, game_state)) => {
            let response = PlayGameResponse {
                success: true,
                swap_id: play_request.swap_id,
                winning_sacp,
                game_state,
            };

            Ok(ok_response(serde_json::to_string(&response)?))
        }
        Err(e) => Ok(bad_request_response(serde_json::to_string(
            &ErrorResponse {
                success: false,
                error: format!("Failed to play game: {}", e),
            },
        )?)),
    }
}

/// Get game state
pub fn get_game_state(
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

    // Extract swap_id from path (assuming format: /games/{swap_id})
    let path_parts: Vec<&str> = request.uri().path().split('/').collect();
    if path_parts.len() < 3 || path_parts[1] != "games" {
        return Ok(bad_request_response(serde_json::to_string(
            &ErrorResponse {
                success: false,
                error: "Invalid path format. Expected /games/{swap_id}".to_string(),
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
    let game_manager = state.game_manager.clone();

    match rt.block_on(async { game_manager.get_game_state(swap_id).await }) {
        Ok(Some(challenge)) => {
            let response = GetGameStateResponse {
                success: true,
                swap_id: challenge.swap_id.clone(),
                alice_partial_tx_hex: challenge.alice_partial_tx_hex.clone(),
                bob_partial_tx_hex: challenge.bob_partial_tx_hex.clone(),
                game_state: challenge.game_state.clone(),
                is_ready: !challenge.alice_partial_tx_hex.is_empty()
                    && challenge.bob_partial_tx_hex.is_empty(),
            };

            Ok(ok_response(serde_json::to_string(&response)?))
        }
        Ok(None) => Ok(not_found_response(serde_json::to_string(&ErrorResponse {
            success: false,
            error: format!("No game found for swap_id: {}", swap_id),
        })?)),
        Err(e) => Ok(internal_server_error_response(serde_json::to_string(
            &ErrorResponse {
                success: false,
                error: format!("Failed to get game state: {}", e),
            },
        )?)),
    }
}

/// Check if game is ready to play
pub fn check_game_ready(
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

    // Extract swap_id from path (assuming format: /games/{swap_id}/ready)
    let path_parts: Vec<&str> = request.uri().path().split('/').collect();
    if path_parts.len() < 4 || path_parts[1] != "games" || path_parts[3] != "ready" {
        return Ok(bad_request_response(serde_json::to_string(
            &ErrorResponse {
                success: false,
                error: "Invalid path format. Expected /games/{swap_id}/ready".to_string(),
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
    let game_manager = state.game_manager.clone();

    match rt.block_on(async { game_manager.is_ready(swap_id).await }) {
        Ok(is_ready) => {
            let response = GameReadyResponse {
                success: true,
                swap_id: swap_id.to_string(),
                is_ready,
            };

            Ok(ok_response(serde_json::to_string(&response)?))
        }
        Err(e) => Ok(internal_server_error_response(serde_json::to_string(
            &ErrorResponse {
                success: false,
                error: format!("Failed to check game readiness: {}", e),
            },
        )?)),
    }
}
