use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

/// Maximum bytes per OP_RETURN (Bitcoin Core default policy limit)
pub const MAX_OP_RETURN_SIZE: usize = 80;

/// Metadata prefix to identify chunked data
/// Format: "IMG" + chunk_index (1 byte) + total_chunks (1 byte)
pub const METADATA_SIZE: usize = 5;

/// Maximum data bytes per chunk (after metadata)
pub const MAX_DATA_PER_CHUNK: usize = MAX_OP_RETURN_SIZE - METADATA_SIZE;

#[derive(Debug)]
pub struct ImageChunk {
    pub index: u8,
    pub total_chunks: u8,
    pub data: Vec<u8>,
}

impl ImageChunk {
    /// Encodes the chunk into bytes ready for OP_RETURN
    /// Format: "IMG" + index + total + data
    pub fn encode(&self) -> Vec<u8> {
        let mut encoded = Vec::with_capacity(METADATA_SIZE + self.data.len());
        encoded.extend_from_slice(b"IMG");
        encoded.push(self.index);
        encoded.push(self.total_chunks);
        encoded.extend_from_slice(&self.data);
        encoded
    }
}

/// Reads a file and splits it into chunks suitable for OP_RETURN outputs
pub fn chunk_file(file_path: &Path) -> Result<Vec<ImageChunk>> {
    let data =
        fs::read(file_path).context(format!("Failed to read file: {}", file_path.display()))?;

    let total_size = data.len();
    let total_chunks = total_size.div_ceil(MAX_DATA_PER_CHUNK) as u8;

    if total_chunks == 0 {
        anyhow::bail!("File is empty");
    }

    println!("File size: {} bytes", total_size);
    println!(
        "Chunks needed: {} (max {} bytes per chunk)",
        total_chunks, MAX_DATA_PER_CHUNK
    );

    let mut chunks = Vec::new();
    for (i, chunk_data) in data.chunks(MAX_DATA_PER_CHUNK).enumerate() {
        chunks.push(ImageChunk {
            index: i as u8,
            total_chunks,
            data: chunk_data.to_vec(),
        });
    }

    Ok(chunks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_encoding() {
        let chunk = ImageChunk {
            index: 0,
            total_chunks: 3,
            data: vec![0xDE, 0xAD, 0xBE, 0xEF],
        };

        let encoded = chunk.encode();
        assert_eq!(&encoded[0..3], b"IMG");
        assert_eq!(encoded[3], 0);
        assert_eq!(encoded[4], 3);
        assert_eq!(&encoded[5..], &[0xDE, 0xAD, 0xBE, 0xEF]);
    }

    #[test]
    fn test_chunk_size() {
        // Verify our constants are correct
        assert_eq!(MAX_DATA_PER_CHUNK, 75);
        assert_eq!(METADATA_SIZE + MAX_DATA_PER_CHUNK, 80);
    }
}
