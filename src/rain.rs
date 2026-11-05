//! The Green Terminal code rain's glyph atlas (Rebuild Prompt, "Green").
//!
//! The 157 GNU Unifont bitmaps in `rain_glyph_data.rs` are packed into a
//! 128x160 single-channel atlas: 16 columns of 8x16 slots, glyph `i` in
//! slot `i` (left to right, top to bottom), the last three slots empty
//! padding. The display-effects pass (`post.wgsl`) reads it to draw the
//! rain; no font is loaded at run time.

#[path = "rain_glyph_data.rs"]
mod data;

pub use data::{GLYPHS, GLYPH_COUNT};

/// Glyph width and height in pixels.
pub const GLYPH_SIZE: (usize, usize) = (8, 16);
/// Slots across the atlas.
pub const ATLAS_COLUMNS: usize = 16;
/// The atlas's width and height in pixels.
pub const ATLAS_SIZE: (usize, usize) = (128, 160);

/// The atlas as one byte per pixel, row by row: 255 where a glyph's bit is
/// set, 0 elsewhere. O(atlas pixels).
pub fn atlas() -> Vec<u8> {
    let (width, height) = ATLAS_SIZE;
    let (glyph_width, glyph_height) = GLYPH_SIZE;
    let mut pixels = vec![0u8; width * height];
    for (index, (_, rows)) in GLYPHS.iter().enumerate() {
        let left = (index % ATLAS_COLUMNS) * glyph_width;
        let top = (index / ATLAS_COLUMNS) * glyph_height;
        for (y, bits) in rows.iter().enumerate() {
            for x in 0..glyph_width {
                // Most significant bit leftmost.
                if bits & (0x80 >> x) != 0 {
                    pixels[(top + y) * width + left + x] = 255;
                }
            }
        }
    }
    pixels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyphs_are_printable_ascii_then_half_width_katakana() {
        let codes: Vec<u32> = GLYPHS.iter().map(|(code, _)| *code).collect();
        let expected: Vec<u32> = (0x21..=0x7E).chain(0xFF61..=0xFF9F).collect();
        assert_eq!(GLYPH_COUNT, 157);
        assert_eq!(codes, expected);
        assert!(GLYPHS.iter().all(|(_, rows)| rows.iter().any(|&r| r != 0)));
    }

    #[test]
    fn atlas_places_each_glyph_in_its_slot_msb_left() {
        let pixels = atlas();
        assert_eq!(pixels.len(), 128 * 160);
        // '!' (slot 0) is a vertical bar in column 4 (bit 0x08).
        let at = |x: usize, y: usize| pixels[y * 128 + x];
        assert_eq!(at(4, 4), 255);
        assert_eq!(at(3, 4), 0);
        // Slot 17 ('2') starts at (8, 16); it matches its bitmap.
        let (_, rows) = GLYPHS[17];
        for (y, bits) in rows.iter().enumerate() {
            for x in 0..8 {
                assert_eq!(at(8 + x, 16 + y) == 255, bits & (0x80 >> x) != 0);
            }
        }
    }

    #[test]
    fn atlas_ends_with_three_empty_padding_slots() {
        let pixels = atlas();
        for slot in 157..160 {
            let (left, top) = ((slot % 16) * 8, (slot / 16) * 16);
            for y in top..top + 16 {
                assert!(pixels[y * 128 + left..y * 128 + left + 8]
                    .iter()
                    .all(|&p| p == 0));
            }
        }
    }
}
