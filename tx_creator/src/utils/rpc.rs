use anyhow::{bail, Context, Result};
use bitcoincore_rpc::{Client, RpcApi};

/// Validates that the RPC connection is working and we're on the expected network
pub fn validate_connection(client: &Client, expected_network: &str) -> Result<()> {
    let blockchain_info = client
        .get_blockchain_info()
        .context("Failed to connect to Bitcoin RPC")?;

    let chain_name = match blockchain_info.chain {
        bitcoin::Network::Bitcoin => "main",
        bitcoin::Network::Testnet => "test",
        bitcoin::Network::Signet => "signet",
        bitcoin::Network::Regtest => "regtest",
        _ => "unknown",
    };

    if chain_name != expected_network {
        bail!(
            "Wrong network: expected {}, got {}",
            expected_network,
            chain_name
        );
    }

    Ok(())
}
