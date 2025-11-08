pub mod chained_op_return;
pub mod p2wsh_fake_multisig;

use anyhow::{bail, Result};
use bitcoincore_rpc::Client;
use std::str::FromStr;

#[derive(Debug, Clone, Copy)]
pub enum Technique {
    ChainedOpReturn,
    P2wshFakeMultisig,
}

impl Technique {
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

impl FromStr for Technique {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "chained-op-return" => Ok(Self::ChainedOpReturn),
            "p2wsh-fake-multisig" => Ok(Self::P2wshFakeMultisig),
            _ => bail!("Unknown technique: {}", s),
        }
    }
}

impl std::fmt::Display for Technique {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ChainedOpReturn => write!(f, "chained-op-return"),
            Self::P2wshFakeMultisig => write!(f, "p2wsh-fake-multisig"),
        }
    }
}

pub trait TechniqueEncoderDecoder {
    fn encode(
        &self,
        data: &[u8],
        client: &Client,
    ) -> Result<(Vec<bitcoin::Transaction>, bitcoin::Txid)>;
    fn decode(&self, txid: &bitcoin::Txid, client: &Client) -> Result<Vec<u8>>;
}
