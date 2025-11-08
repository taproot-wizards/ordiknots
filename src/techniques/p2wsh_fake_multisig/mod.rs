mod decode;
mod encode;

use anyhow::Result;
use bitcoincore_rpc::Client;

use crate::techniques::TechniqueEncoderDecoder;

pub struct P2wshFakeMultisig;

impl TechniqueEncoderDecoder for P2wshFakeMultisig {
    fn encode(
        &self,
        data: &[u8],
        client: &Client,
    ) -> Result<(Vec<bitcoin::Transaction>, bitcoin::Txid)> {
        encode::encode(data, client)
    }

    fn decode(&self, txid: &bitcoin::Txid, client: &Client) -> Result<Vec<u8>> {
        decode::decode(txid, client)
    }
}
