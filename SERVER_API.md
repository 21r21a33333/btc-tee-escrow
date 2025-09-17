# BTC Escrow TEE Server API

This document describes the HTTP API for the BTC Escrow TEE-compatible server.

## Overview

The server is designed to run in a Trusted Execution Environment (TEE) and provides endpoints for managing Bitcoin escrow swaps and coin flip games. It uses a custom HTTP implementation built on top of `std::net::TcpListener` and `TcpStream` to avoid dependencies on external web frameworks that might not be compatible with TEE environments.

## Base URL

```
http://localhost:8080
```

## Authentication

Currently, the server does not implement authentication. In a production TEE environment, you would typically implement authentication mechanisms appropriate for your specific TEE platform.

## Endpoints

### Health Check

#### GET /health

Check if the server is running and healthy.

**Response:**

```json
{
  "status": "healthy",
  "service": "btc-escrow-tee",
  "timestamp": 1640995200
}
```

### Swap Management

#### POST /swaps

Create a new swap between Alice and Bob.

**Request Body:**

```json
{
  "alice_pubkey": "alice_public_key_hex",
  "bob_pubkey": "bob_public_key_hex"
}
```

**Response:**

```json
{
  "success": true,
  "swap_id": "generated_swap_id_hash",
  "alice_pubkey": "alice_public_key_hex",
  "bob_pubkey": "bob_public_key_hex",
  "alice_wallet_addr": "alice_wallet_address",
  "bob_wallet_addr": "bob_wallet_address"
}
```

**Error Response:**

```json
{
  "success": false,
  "error": "Error description"
}
```

#### GET /swaps/{swap_id}

Retrieve a swap by its ID.

**Response:**

```json
{
  "success": true,
  "swap_id": "swap_id_hash",
  "alice_pubkey": "alice_public_key_hex",
  "bob_pubkey": "bob_public_key_hex",
  "alice_wallet_addr": "alice_wallet_address",
  "bob_wallet_addr": "bob_wallet_address",
  "challenges": []
}
```

#### GET /swaps

List all swaps (placeholder implementation).

**Response:**

```json
{
  "success": true,
  "message": "List swaps endpoint - to be implemented",
  "swaps": []
}
```

### Game Management

#### POST /games

Initialize a new coin flip game for a swap (Alice's turn).

**Request Body:**

```json
{
  "sacp": "alice_signed_partial_transaction_hex",
  "swap_id": "swap_id_hash"
}
```

**Response:**

```json
{
  "success": true,
  "swap_id": "swap_id_hash",
  "message": "Game initialized successfully"
}
```

#### POST /games/play

Play the coin flip game (Bob's turn).

**Request Body:**

```json
{
  "sacp": "bob_signed_partial_transaction_hex",
  "swap_id": "swap_id_hash"
}
```

**Response:**

```json
{
  "success": true,
  "swap_id": "swap_id_hash",
  "winning_sacp": "winning_participant_sacp",
  "game_state": "Heads" // or "Tails"
}
```

#### GET /games/{swap_id}

Get the current state of a game.

**Response:**

```json
{
  "success": true,
  "swap_id": "swap_id_hash",
  "alice_partial_tx_hex": "alice_sacp",
  "bob_partial_tx_hex": "bob_sacp_or_empty",
  "game_state": "None", // "None", "Heads", or "Tails"
  "is_ready": true
}
```

#### GET /games/{swap_id}/ready

Check if a game is ready for Bob to play (Alice has submitted her SACp).

**Response:**

```json
{
  "success": true,
  "swap_id": "swap_id_hash",
  "is_ready": true
}
```

## Game Flow

1. **Create Swap**: Use `POST /swaps` to create a swap between Alice and Bob
2. **Initialize Game**: Alice calls `POST /games` with her SACp to start the coin flip
3. **Check Readiness**: Bob can check `GET /games/{swap_id}/ready` to see if he can play
4. **Play Game**: Bob calls `POST /games/play` with his SACp to determine the winner
5. **Get Results**: Both parties can check `GET /games/{swap_id}` to see the final state

## Error Handling

All endpoints return JSON responses with a `success` field indicating whether the operation succeeded. Error responses include an `error` field with a description of what went wrong.

Common HTTP status codes:

- `200 OK`: Successful operation
- `400 Bad Request`: Invalid request data
- `404 Not Found`: Resource not found
- `500 Internal Server Error`: Server error

## Concurrency

The server is designed to handle concurrent requests using a thread-per-connection model. Each client connection is handled in a separate thread, allowing multiple clients to interact with the server simultaneously.

## TEE Compatibility

This server implementation is designed to be compatible with Trusted Execution Environments by:

1. Using only standard library networking primitives (`TcpListener`, `TcpStream`)
2. Avoiding external web frameworks that might not be available in TEE environments
3. Implementing a simple HTTP parser and router
4. Using thread-based concurrency instead of async runtimes that might not be available

## Configuration

The server can be configured by modifying the constants in `src/main.rs`:

- `listen_addr`: The address and port to listen on
- `electrum_url`: The Electrum server URL for Bitcoin network operations
- `cosigner_pubkey`: The cosigner's public key for trade lock scripts
- `NETWORK`: The Bitcoin network (Regtest, Testnet, or Mainnet)

## Example Usage

### Creating a Swap

```bash
curl -X POST http://localhost:8080/swaps \
  -H "Content-Type: application/json" \
  -d '{
    "alice_pubkey": "alice_public_key_hex",
    "bob_pubkey": "bob_public_key_hex"
  }'
```

### Initializing a Game

```bash
curl -X POST http://localhost:8080/games \
  -H "Content-Type: application/json" \
  -d '{
    "sacp": "alice_signed_partial_transaction_hex",
    "swap_id": "swap_id_hash"
  }'
```

### Playing a Game

```bash
curl -X POST http://localhost:8080/games/play \
  -H "Content-Type: application/json" \
  -d '{
    "sacp": "bob_signed_partial_transaction_hex",
    "swap_id": "swap_id_hash"
  }'
```
