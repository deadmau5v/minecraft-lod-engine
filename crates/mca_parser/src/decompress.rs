use anyhow::{bail, Result};
use flate2::read::{GzDecoder, ZlibDecoder};
use std::cell::RefCell;
use std::io::Read;

thread_local! {
    static DECOMPRESS_SCRATCH: RefCell<Vec<u8>> = RefCell::new(Vec::with_capacity(256 * 1024));
}

pub fn decompress_chunk_payload(
    compressed: &[u8],
    compression_type: u8,
    out: &mut Vec<u8>,
) -> Result<()> {
    out.clear();
    match compression_type {
        1 => {
            // Gzip
            let mut decoder = GzDecoder::new(compressed);
            decoder.read_to_end(out)?;
            Ok(())
        }
        2 => {
            // Zlib
            let mut decoder = ZlibDecoder::new(compressed);
            decoder.read_to_end(out)?;
            Ok(())
        }
        3 => {
            // Uncompressed
            out.extend_from_slice(compressed);
            Ok(())
        }
        4 => {
            // LZ4
            let decompressed = lz4_flex::decompress_size_prepended(compressed)
                .map_err(|e| anyhow::anyhow!("LZ4 decompression error: {:?}", e))?;
            *out = decompressed;
            Ok(())
        }
        other => {
            bail!("Unsupported MCA compression type: {}", other);
        }
    }
}

pub fn with_decompress_scratch<F, R>(f: F) -> R
where
    F: FnOnce(&mut Vec<u8>) -> R,
{
    DECOMPRESS_SCRATCH.with(|cell| {
        let mut buf = cell.borrow_mut();
        f(&mut buf)
    })
}
