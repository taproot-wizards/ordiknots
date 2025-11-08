pub mod chained_op_return;
pub mod p2wsh_fake_multisig;

use anyhow::{bail, Result};
use bitcoincore_rpc::Client;

pub trait Technique {
    fn encode(&self, data: &[u8], client: &Client) -> Result<(Vec<bitcoin::Transaction>, bitcoin::Txid)>;
    fn decode(&self, txid: &bitcoin::Txid, client: &Client) -> Result<Vec<u8>>;
}

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
            _ => bail!(
                "Unknown technique: {}. Valid options: chained-op-return, p2wsh-fake-multisig",
                s
            ),
        }
    }

    /// Encode data using this technique
    pub fn encode(
        &self,
        data: &[u8],
        client: &Client,
    ) -> Result<(Vec<bitcoin::Transaction>, bitcoin::Txid)> {
        match self {
            Self::ChainedOpReturn => {
                let technique = chained_op_return::ChainedOpReturn;
                technique.encode(data, client)
            }
            Self::P2wshFakeMultisig => {
                let technique = p2wsh_fake_multisig::P2wshFakeMultisig;
                technique.encode(data, client)
            }
        }
    }

    /// Decode data using this technique
    pub fn decode(&self, txid: &bitcoin::Txid, client: &Client) -> Result<Vec<u8>> {
        match self {
            Self::ChainedOpReturn => {
                let technique = chained_op_return::ChainedOpReturn;
                technique.decode(txid, client)
            }
            Self::P2wshFakeMultisig => {
                let technique = p2wsh_fake_multisig::P2wshFakeMultisig;
                technique.decode(txid, client)
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
