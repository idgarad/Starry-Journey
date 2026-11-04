//! Snapshot mode helpers (owner decision, 2026-11-04: let the agent see the
//! app without a window). `--snapshot` renders chosen moments of a run
//! through the real GPU shader into an offscreen texture and saves each as
//! a PNG, so visual work can be checked without opening a window or taking
//! focus. The renderer side lives in `main.rs`; this module holds the pure
//! parts: PNG encoding and run folder names.
//!
//! The PNG writer is deliberately minimal (no dependency): 8-bit RGBA, no
//! filtering, and "stored" (uncompressed) deflate blocks inside the zlib
//! stream. Files are about four bytes per pixel, which is fine for test
//! frames that are deleted with `target/`.

/// Where snapshot runs go unless `--snapshot-dir` says otherwise: inside
/// `target/`, which git ignores and `cargo clean` removes.
pub const DEFAULT_SNAPSHOT_DIR: &str = "target/snapshots";

/// The CRC-32 (ISO-HDLC, as PNG uses) of `bytes`. O(n).
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

/// The Adler-32 checksum zlib appends to its stream. O(n).
fn adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in bytes.chunks(5552) {
        for &byte in chunk {
            a += u32::from(byte);
            b += a;
        }
        a %= 65_521;
        b %= 65_521;
    }
    (b << 16) | a
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// Encodes a `width x height` image of 8-bit RGBA pixels (rows top to
/// bottom, `width * height * 4` bytes) as a PNG file. O(pixels).
///
/// # Panics
///
/// Panics if `rgba` is not exactly `width * height * 4` bytes.
pub fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let row = width as usize * 4;
    assert_eq!(rgba.len(), row * height as usize, "RGBA size");
    // Each scanline starts with filter type 0 (none).
    let mut raw = Vec::with_capacity((row + 1) * height as usize);
    for line in rgba.chunks(row.max(1)).take(height as usize) {
        raw.push(0);
        raw.extend_from_slice(line);
    }
    // zlib header (deflate, 32K window, no preset dictionary), stored
    // blocks of at most 65535 bytes, then the Adler-32 of the raw data.
    let mut zlib = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = if raw.is_empty() {
        vec![&[]]
    } else {
        raw.chunks(65_535).collect()
    };
    for (i, block) in blocks.iter().enumerate() {
        zlib.push(u8::from(i + 1 == blocks.len()));
        let len = block.len() as u16;
        zlib.extend_from_slice(&len.to_le_bytes());
        zlib.extend_from_slice(&(!len).to_le_bytes());
        zlib.extend_from_slice(block);
    }
    zlib.extend_from_slice(&adler32(&raw).to_be_bytes());

    let mut out = vec![0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];
    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    // 8 bits per channel, color type 6 (RGBA), deflate, no filter set
    // choice, no interlace.
    header.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &header);
    chunk(&mut out, b"IDAT", &zlib);
    chunk(&mut out, b"IEND", &[]);
    out
}

/// A run folder name, `YYYYMMDD-HHMMSS` in UTC, for `unix_seconds` since
/// 1970. O(1).
pub fn run_id(unix_seconds: u64) -> String {
    let days = (unix_seconds / 86_400) as i64;
    let rest = unix_seconds % 86_400;
    // Days since 1970-01-01 to a civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}{month:02}{day:02}-{:02}{:02}{:02}",
        rest / 3600,
        rest / 60 % 60,
        rest % 60
    )
}

/// The file name for a snapshot `seconds` into the run, sortable by time:
/// `t000030.0s.png`.
pub fn frame_name(seconds: f64) -> String {
    format!("t{:08.1}s.png", seconds.max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_and_adler_match_known_values() {
        assert_eq!(crc32(b"IEND"), 0xae42_6082);
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
        assert_eq!(adler32(b"Wikipedia"), 0x11e6_0398);
        assert_eq!(adler32(&[]), 1);
    }

    #[test]
    fn png_has_the_signature_header_and_end() {
        let png = encode_png(2, 1, &[255, 0, 0, 255, 0, 0, 255, 128]);
        assert_eq!(
            &png[..8],
            &[0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n']
        );
        assert_eq!(&png[12..16], b"IHDR");
        assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), 2);
        assert_eq!(u32::from_be_bytes(png[20..24].try_into().unwrap()), 1);
        assert_eq!(&png[24..29], &[8, 6, 0, 0, 0]);
        assert_eq!(
            &png[png.len() - 12..],
            &[0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xae, 0x42, 0x60, 0x82]
        );
    }

    #[test]
    fn png_stores_large_images_in_several_blocks() {
        // 200 x 100 RGBA is 80,100 raw bytes: two stored blocks.
        let rgba = vec![7u8; 200 * 100 * 4];
        let png = encode_png(200, 100, &rgba);
        let idat = png.windows(4).position(|w| w == b"IDAT").unwrap();
        let len = u32::from_be_bytes(png[idat - 4..idat].try_into().unwrap()) as usize;
        // header 2 + two block headers 5 each + data + Adler 4.
        assert_eq!(len, 2 + 5 + 5 + (200 * 4 + 1) * 100 + 4);
        let first_block = png[idat + 6];
        assert_eq!(first_block, 0, "the first block is not the last");
    }

    #[test]
    #[should_panic(expected = "RGBA size")]
    fn png_rejects_the_wrong_buffer_size() {
        let _ = encode_png(2, 2, &[0; 4]);
    }

    #[test]
    fn run_ids_are_utc_dates_and_times() {
        assert_eq!(run_id(0), "19700101-000000");
        // 2026-11-04 12:34:56 UTC.
        assert_eq!(run_id(1_793_795_696), "20261104-123456");
        // 2000-02-29 (a leap day) 23:59:59.
        assert_eq!(run_id(951_868_799), "20000229-235959");
    }

    #[test]
    fn frame_names_sort_by_time() {
        assert_eq!(frame_name(30.0), "t000030.0s.png");
        assert!(frame_name(9.5) < frame_name(600.0));
        assert_eq!(frame_name(-1.0), "t000000.0s.png");
    }
}
