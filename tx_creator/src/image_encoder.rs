use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

/// Maximum bytes per OP_RETURN (Bitcoin Knots default policy limit)
/// This is the total scriptPubKey size, including OP_RETURN opcode and push opcode
/// So actual data capacity is 40 bytes (42 - 1 for OP_RETURN - 1 for push opcode)
pub const MAX_OP_RETURN_SIZE: usize = 40;

/// Metadata prefix to identify chunked data
/// Format: "IMG" + chunk_index (1 byte) + total_chunks (1 byte) + file_size (2 bytes for first chunk only)
pub const METADATA_SIZE: usize = 5;
pub const FIRST_CHUNK_METADATA_SIZE: usize = 7; // Includes 2-byte file size

/// Maximum data bytes per chunk (after metadata)
pub const MAX_DATA_PER_CHUNK: usize = MAX_OP_RETURN_SIZE - METADATA_SIZE;
pub const FIRST_CHUNK_MAX_DATA: usize = MAX_OP_RETURN_SIZE - FIRST_CHUNK_METADATA_SIZE;

#[derive(Debug)]
pub struct ImageChunk {
    pub index: u8,
    pub total_chunks: u8,
    pub file_size: Option<u16>, // Only set for first chunk
    pub data: Vec<u8>,
}

impl ImageChunk {
    /// Encodes the chunk into bytes ready for OP_RETURN
    /// Format: "IMG" + index + total + [file_size (first chunk only)] + data
    pub fn encode(&self) -> Vec<u8> {
        let mut encoded = Vec::new();
        encoded.extend_from_slice(b"IMG");
        encoded.push(self.index);
        encoded.push(self.total_chunks);

        // First chunk includes file size
        if let Some(size) = self.file_size {
            encoded.extend_from_slice(&size.to_le_bytes());
        }

        encoded.extend_from_slice(&self.data);
        encoded
    }
}

/// Minimum data bytes to ensure transaction meets minimum size (85 bytes)
/// With metadata overhead, 30 bytes of OP_RETURN data is enough
const MIN_LAST_CHUNK_SIZE: usize = 30;

/// Reads a file and splits it into chunks suitable for OP_RETURN outputs
pub fn chunk_file(file_path: &Path) -> Result<Vec<ImageChunk>> {
    let data =
        fs::read(file_path).context(format!("Failed to read file: {}", file_path.display()))?;

    let total_size = data.len();
    if total_size > 65535 {
        anyhow::bail!("File too large (max 65535 bytes)");
    }

    // First chunk has less space due to file size metadata
    let first_chunk_size = FIRST_CHUNK_MAX_DATA.min(total_size);
    let remaining_size = total_size.saturating_sub(first_chunk_size);
    let total_chunks_usize = if remaining_size > 0 {
        1 + remaining_size.div_ceil(MAX_DATA_PER_CHUNK)
    } else {
        1
    };

    if total_chunks_usize == 0 {
        anyhow::bail!("File is empty");
    }

    if total_chunks_usize > 255 {
        anyhow::bail!(
            "File too large: needs {} chunks but maximum is 255 (max ~{} bytes with current chunk size)",
            total_chunks_usize,
            FIRST_CHUNK_MAX_DATA + (254 * MAX_DATA_PER_CHUNK)
        );
    }

    let total_chunks = total_chunks_usize as u8;

    println!("File size: {} bytes", total_size);
    println!(
        "Chunks needed: {} (first chunk: {} bytes, remaining: {} bytes each)",
        total_chunks, FIRST_CHUNK_MAX_DATA, MAX_DATA_PER_CHUNK
    );

    let mut chunks = Vec::new();
    let mut offset = 0;

    // First chunk with file size
    let first_data_len = FIRST_CHUNK_MAX_DATA.min(total_size);
    chunks.push(ImageChunk {
        index: 0,
        total_chunks,
        file_size: Some(total_size as u16),
        data: data[..first_data_len].to_vec(),
    });
    offset += first_data_len;

    // Remaining chunks
    let mut chunk_index: u8 = 1;
    while offset < total_size {
        let chunk_size = MAX_DATA_PER_CHUNK.min(total_size - offset);
        let mut chunk_data = data[offset..offset + chunk_size].to_vec();

        // Pad last chunk if it's too small to meet minimum transaction size
        let is_last = offset + chunk_size >= total_size;
        if is_last && chunk_data.len() < MIN_LAST_CHUNK_SIZE {
            let padding_needed = MIN_LAST_CHUNK_SIZE - chunk_data.len();
            chunk_data.extend(vec![0u8; padding_needed]);
            println!("  Padding last chunk with {} null bytes", padding_needed);
        }

        chunks.push(ImageChunk {
            index: chunk_index,
            total_chunks,
            file_size: None,
            data: chunk_data,
        });

        offset += chunk_size;
        if offset < total_size {
            chunk_index = chunk_index.checked_add(1).expect("Too many chunks (max 255)");
        }
    }

    Ok(chunks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_encoding() {
        // Test first chunk with file size
        let chunk = ImageChunk {
            index: 0,
            total_chunks: 3,
            file_size: Some(200),
            data: vec![0xDE, 0xAD, 0xBE, 0xEF],
        };

        let encoded = chunk.encode();
        assert_eq!(&encoded[0..3], b"IMG");
        assert_eq!(encoded[3], 0); // index
        assert_eq!(encoded[4], 3); // total_chunks
        assert_eq!(u16::from_le_bytes([encoded[5], encoded[6]]), 200); // file_size
        assert_eq!(&encoded[7..], &[0xDE, 0xAD, 0xBE, 0xEF]);

        // Test subsequent chunk without file size
        let chunk2 = ImageChunk {
            index: 1,
            total_chunks: 3,
            file_size: None,
            data: vec![0xCA, 0xFE],
        };

        let encoded2 = chunk2.encode();
        assert_eq!(&encoded2[0..3], b"IMG");
        assert_eq!(encoded2[3], 1);
        assert_eq!(encoded2[4], 3);
        assert_eq!(&encoded2[5..], &[0xCA, 0xFE]);
    }

    #[test]
    fn test_chunk_size() {
        // Verify our constants are correct
        assert_eq!(MAX_DATA_PER_CHUNK, 35);
        assert_eq!(METADATA_SIZE + MAX_DATA_PER_CHUNK, 40);
        assert_eq!(FIRST_CHUNK_MAX_DATA, 33);
        assert_eq!(FIRST_CHUNK_METADATA_SIZE + FIRST_CHUNK_MAX_DATA, 40);
    }
}
