pub mod chained_op_return;
pub mod p2wsh_fake_multisig;

use anyhow::{bail, Result};
use bitcoincore_rpc::Client;
use std::str::FromStr;
use strum::IntoEnumIterator;

#[derive(Debug, Clone, Copy, strum::EnumIter)]
#[repr(u8)]
pub enum Technique {
    ChainedOpReturn = 1,
    P2wshFakeMultisig = 2,
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

    pub fn detect(&self, tx: &bitcoin::Transaction) -> bool {
        match self {
            Self::ChainedOpReturn => {
                let technique = chained_op_return::ChainedOpReturn;
                technique.detect(tx)
            }
            Self::P2wshFakeMultisig => {
                let technique = p2wsh_fake_multisig::P2wshFakeMultisig;
                technique.detect(tx)
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
    fn detect(&self, tx: &bitcoin::Transaction) -> bool;
}

/// Detects if a transaction contains encoded data and returns the technique type
pub fn detect_technique(tx: &bitcoin::Transaction) -> Option<Technique> {
    Technique::iter().find(|technique| technique.detect(tx))
}
