//! A minimal PNG writer.
//!
//! Screenshots must be viewable without pulling in an image crate, so this
//! writes a valid PNG using stored (uncompressed) deflate blocks: no zlib
//! dependency, ~100 lines, deterministic output.

use std::io::{self, Write};
use std::path::Path;

const SIGNATURE: [u8; 8] = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];

/// Encodes 0x00RRGGBB pixels as an 8-bit RGB PNG.
pub fn encode_rgb(width: u32, height: u32, pixels: &[u32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(pixels.len() * 3 + 1024);
    out.extend_from_slice(&SIGNATURE);

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.push(8); // bit depth
    ihdr.push(2); // colour type: truecolour
    ihdr.push(0); // deflate
    ihdr.push(0); // adaptive filtering
    ihdr.push(0); // no interlace
    write_chunk(&mut out, b"IHDR", &ihdr);

    // Raw scanlines: one filter byte (0 = none) then RGB triples.
    let mut raw = Vec::with_capacity(pixels.len() * 3 + height as usize);
    for y in 0..height as usize {
        raw.push(0);
        for x in 0..width as usize {
            let p = pixels.get(y * width as usize + x).copied().unwrap_or(0);
            raw.push((p >> 16) as u8);
            raw.push((p >> 8) as u8);
            raw.push(p as u8);
        }
    }

    let mut zlib = Vec::with_capacity(raw.len() + raw.len() / 65535 * 5 + 16);
    zlib.push(0x78); // CMF: deflate, 32 KiB window
    zlib.push(0x01); // FLG: no dictionary, fastest
    let mut offset = 0usize;
    while offset < raw.len() {
        let chunk = (raw.len() - offset).min(65535);
        let final_block = offset + chunk >= raw.len();
        zlib.push(u8::from(final_block)); // BFINAL, BTYPE = 00 (stored)
        zlib.extend_from_slice(&(chunk as u16).to_le_bytes());
        zlib.extend_from_slice(&(!(chunk as u16)).to_le_bytes());
        zlib.extend_from_slice(&raw[offset..offset + chunk]);
        offset += chunk;
    }
    zlib.extend_from_slice(&adler32(&raw).to_be_bytes());
    write_chunk(&mut out, b"IDAT", &zlib);
    write_chunk(&mut out, b"IEND", &[]);
    out
}

pub fn save_rgb(path: &Path, width: u32, height: u32, pixels: &[u32]) -> io::Result<()> {
    if let Some(dir) = path.parent()
        && !dir.as_os_str().is_empty()
    {
        std::fs::create_dir_all(dir)?;
    }
    let bytes = encode_rgb(width, height, pixels);
    let mut file = std::fs::File::create(path)?;
    file.write_all(&bytes)
}

fn write_chunk(out: &mut Vec<u8>, kind: &[u8; 4], payload: &[u8]) {
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(payload);
    let mut crc_input = Vec::with_capacity(4 + payload.len());
    crc_input.extend_from_slice(kind);
    crc_input.extend_from_slice(payload);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

fn adler32(data: &[u8]) -> u32 {
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for &byte in data {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adler_matches_known_value() {
        // zlib's documented example: adler32("Wikipedia") == 0x11E60398
        assert_eq!(adler32(b"Wikipedia"), 0x11e6_0398);
    }

    #[test]
    fn crc_matches_known_value() {
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
    }

    #[test]
    fn encoder_emits_expected_structure() {
        let px = vec![0x00ff_0000u32, 0x0000_ff00, 0x0000_00ff, 0x00ff_ffff];
        let png = encode_rgb(2, 2, &px);
        assert_eq!(&png[..8], &SIGNATURE);
        assert_eq!(&png[12..16], b"IHDR");
        assert!(png.windows(4).any(|w| w == b"IDAT"));
        assert!(png.windows(4).any(|w| w == b"IEND"));
        assert_eq!(u32::from_be_bytes([png[16], png[17], png[18], png[19]]), 2);
    }
}
