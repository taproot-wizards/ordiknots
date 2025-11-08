use anyhow::{Context, Result};
use bitcoincore_rpc::{Auth, Client, RpcApi};
use std::fs;
use std::path::PathBuf;

use ordiknots::techniques::TechniqueType;

/// Integration test for chained OP_RETURN encoding/decoding
///
/// NOTE: Run these tests with --test-threads=1 to avoid UTXO conflicts:
/// cargo test --test roundtrip_test -- --ignored --test-threads=1 --nocapture
#[test]
#[ignore]
fn test_chained_op_return_roundtrip() -> Result<()> {
    test_roundtrip(TechniqueType::ChainedOpReturn, "chained_op_return")
}

/// Integration test for P2WSH CHECKMULTISIG encoding/decoding
#[test]
#[ignore] // Run with: cargo test --test roundtrip_test -- --ignored
fn test_p2wsh_fake_multisig_roundtrip() -> Result<()> {
    test_roundtrip(TechniqueType::P2wshFakeMultisig, "p2wsh_fake_multisig")
}

/// Generic roundtrip test that encodes punk.png, broadcasts transactions,
/// then decodes and verifies the result matches the original
fn test_roundtrip(technique: TechniqueType, test_name: &str) -> Result<()> {
    // Setup
    println!("\n=== Testing {} roundtrip ===", test_name);

    let input_file = PathBuf::from("test_data/punk.png");
    let output_file = PathBuf::from(format!("/tmp/punk_decoded_{}.png", test_name));

    // Ensure input file exists
    if !input_file.exists() {
        eprintln!(
            "Skipping test: punk.png not found at {:?}",
            input_file.canonicalize()
        );
        return Ok(());
    }

    // Read original file
    let original_data = fs::read(&input_file).context("Failed to read punk.png")?;
    println!("Original file size: {} bytes", original_data.len());

    // Connect to Bitcoin Core RPC
    let client = Client::new(
        "http://localhost:18443",
        Auth::UserPass("mempool".to_string(), "mempool".to_string()),
    )
    .context("Failed to connect to Bitcoin Core RPC")?;

    // Verify we're on regtest
    let blockchain_info = client
        .get_blockchain_info()
        .context("Failed to get blockchain info")?;

    let chain_name = match blockchain_info.chain {
        bitcoin::Network::Regtest => "regtest",
        _ => "not-regtest",
    };

    assert_eq!(
        chain_name, "regtest",
        "Must be running on regtest network"
    );

    println!("Connected to Bitcoin Core on regtest");
    println!("Using technique: {}", technique);

    // Step 1: Encode and broadcast the file
    let txid = technique
        .encode(&client, &input_file, true)
        .context("Failed to encode and broadcast file")?;

    println!("\nTransaction TXID: {}", txid);

    // Step 2: Decode from blockchain using TXID
    println!("\nDecoding from blockchain...");
    technique
        .decode(&client, &txid, &output_file)
        .context("Failed to decode from blockchain")?;

    // Step 3: Verify decoded file matches original
    let decoded_data = fs::read(&output_file).context("Failed to read decoded file")?;
    println!("Decoded file size: {} bytes", decoded_data.len());

    assert_eq!(
        original_data.len(),
        decoded_data.len(),
        "{} test: File sizes don't match",
        test_name
    );

    assert_eq!(
        original_data, decoded_data,
        "{} test: File contents don't match",
        test_name
    );

    println!("\n✓ {} roundtrip test PASSED - files match!", test_name);

    // Cleanup
    fs::remove_file(&output_file).ok();

    Ok(())
}
