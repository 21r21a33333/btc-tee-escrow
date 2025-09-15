// validate confirmed utxos

// validate tx's in mempool

use crate::btc::ElectrsClient;

pub struct validator {
    client: ElectrsClient,
}
impl validator {
    pub fn new(client: ElectrsClient) -> Self {
        Self { client }
    }
}
