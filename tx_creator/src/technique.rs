use anyhow::Result;
use bitcoincore_rpc::Client;
use std::path::Path;

/// Trait that all embedding techniques must implement
pub trait Technique {
    /// Encodes a file into Bitcoin transaction(s)
    ///
    /// # Arguments
    /// * `client` - RPC client for interacting with Bitcoin Core
    /// * `file_path` - Path to the file to encode
    /// * `broadcast` - Whether to broadcast transactions or just print them
    ///
    /// # Returns
    /// The transaction ID that can be used for decoding (typically the first/main tx)
    fn encode(&self, client: &Client, file_path: &Path, broadcast: bool) -> Result<bitcoin::Txid>;

    /// Decodes a file from Bitcoin transaction(s)
    ///
    /// # Arguments
    /// * `client` - RPC client for interacting with Bitcoin Core
    /// * `txid` - The transaction ID to start decoding from
    /// * `output_path` - Path where the decoded file should be written
    fn decode(&self, client: &Client, txid: &bitcoin::Txid, output_path: &Path) -> Result<()>;
}
