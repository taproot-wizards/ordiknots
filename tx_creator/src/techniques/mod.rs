pub mod chained_op_return;
pub mod p2wsh_fake_multisig;

use anyhow::{bail, Result};
use bitcoincore_rpc::Client;
use std::path::Path;
use crate::technique::Technique;

/// Enum of available embedding techniques
#[derive(Debug, Clone, Copy)]
pub enum TechniqueType {
    ChainedOpReturn,
    P2wshFakeMultisig,
}

impl TechniqueType {
    /// Parse technique type from string
    pub fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "chained-op-return" | "op-return" | "op_return" => Ok(Self::ChainedOpReturn),
            "p2wsh-fake-multisig" | "p2wsh" | "multisig" => Ok(Self::P2wshFakeMultisig),
            _ => bail!("Unknown technique: {}. Valid options: chained-op-return, p2wsh-fake-multisig", s),
        }
    }

    /// Encode a file using this technique
    pub fn encode(&self, client: &Client, file_path: &Path, broadcast: bool) -> Result<bitcoin::Txid> {
        match self {
            Self::ChainedOpReturn => {
                let technique = chained_op_return::ChainedOpReturn;
                technique.encode(client, file_path, broadcast)
            }
            Self::P2wshFakeMultisig => {
                let technique = p2wsh_fake_multisig::P2wshFakeMultisig;
                technique.encode(client, file_path, broadcast)
            }
        }
    }

    /// Decode a file using this technique
    pub fn decode(&self, client: &Client, txid: &bitcoin::Txid, output_path: &Path) -> Result<()> {
        match self {
            Self::ChainedOpReturn => {
                let technique = chained_op_return::ChainedOpReturn;
                technique.decode(client, txid, output_path)
            }
            Self::P2wshFakeMultisig => {
                let technique = p2wsh_fake_multisig::P2wshFakeMultisig;
                technique.decode(client, txid, output_path)
            }
        }
    }
}

impl std::fmt::Display for TechniqueType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ChainedOpReturn => write!(f, "chained-op-return"),
            Self::P2wshFakeMultisig => write!(f, "p2wsh-fake-multisig"),
        }
    }
}
