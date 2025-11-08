use anyhow::Result;
use bitcoincore_rpc::Client;
use std::path::Path;

pub trait Technique {
    fn encode(&self, client: &Client, file_path: &Path, broadcast: bool) -> Result<bitcoin::Txid>;
    fn decode(&self, client: &Client, txid: &bitcoin::Txid, output_path: &Path) -> Result<()>;
}
