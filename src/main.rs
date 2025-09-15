use ::bitcoin::Network;
mod btc;
mod models;
mod services;
mod storage;

// network ideally should be in config
pub const NETWORK: Network = Network::Regtest;

fn main() {}
