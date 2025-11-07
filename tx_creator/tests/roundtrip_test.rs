use anyhow::{Context, Result};
use bitcoincore_rpc::{Auth, Client, RpcApi};
use std::fs;
use std::path::PathBuf;

// Import from the main crate
use tx_creator::image_decoder;
use tx_creator::tx_builder::handle_file_mode;

/// Integration test that encodes punk.png, broadcasts transactions,
/// then decodes and verifies the result matches the original
#[test]
#[ignore] // Run with: cargo test --test roundtrip_test -- --ignored
fn test_encode_decode_roundtrip() -> Result<()> {
    // Setup
    let input_file = PathBuf::from("test_data/punk.png");
    let output_file = PathBuf::from("/tmp/punk_decoded.png");

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
    let rpc = Client::new(
        "http://localhost:18443",
        Auth::UserPass("mempool".to_string(), "mempool".to_string()),
    )
    .context("Failed to connect to Bitcoin Core RPC")?;

    // Verify we're on regtest
    let blockchain_info = rpc
        .get_blockchain_info()
        .context("Failed to get blockchain info")?;

    assert_eq!(
        blockchain_info.chain.to_string(),
        "regtest",
        "Must be running on regtest network"
    );

    println!("Connected to Bitcoin Core on regtest");

    // Step 1: Encode and broadcast the file
    let first_txid = handle_file_mode(&rpc, &input_file, 0, true)
        .context("Failed to encode and broadcast file")?;

    println!("\nFirst TXID: {}", first_txid);

    // Step 2: Decode from blockchain using first TXID
    println!("\nDecoding from blockchain...");
    image_decoder::decode_from_blockchain(&rpc, &first_txid, &output_file)
        .context("Failed to decode from blockchain")?;

    // Step 3: Verify decoded file matches original
    let decoded_data = fs::read(&output_file).context("Failed to read decoded file")?;
    println!("Decoded file size: {} bytes", decoded_data.len());

    assert_eq!(
        original_data.len(),
        decoded_data.len(),
        "File sizes don't match"
    );

    assert_eq!(original_data, decoded_data, "File contents don't match");

    println!("\n✓ Round-trip test PASSED - files match!");

    // Cleanup
    fs::remove_file(&output_file).ok();

    Ok(())
}
