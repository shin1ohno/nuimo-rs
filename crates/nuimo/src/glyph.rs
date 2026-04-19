use crate::gatt::{LED_BITMAP_BYTES, LED_COLS, LED_FADE_FLAG, LED_ROWS};

/// Display transition effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayTransition {
    Immediate,
    CrossFade,
}

/// Options for displaying a glyph.
#[derive(Debug, Clone)]
pub struct DisplayOptions {
    /// Brightness 0.0-1.0.
    pub brightness: f64,
    /// Auto-clear timeout in milliseconds (max 25500).
    pub timeout_ms: u32,
    /// Transition effect.
    pub transition: DisplayTransition,
}

impl Default for DisplayOptions {
    fn default() -> Self {
        Self {
            brightness: 1.0,
            timeout_ms: 2000,
            transition: DisplayTransition::CrossFade,
        }
    }
}

/// A 9x9 LED glyph for the Nuimo display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Glyph {
    /// Each row is a 9-bit value (bit 8 = leftmost pixel).
    rows: [u16; LED_ROWS],
}

impl Glyph {
    /// Create a glyph from a string representation.
    ///
    /// Each line represents a row (up to 9 lines). Use `*` for on, anything else for off.
    /// Lines are separated by newlines.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        let mut rows = [0u16; LED_ROWS];
        for (row_idx, line) in s.lines().enumerate() {
            if row_idx >= LED_ROWS {
                break;
            }
            let mut val = 0u16;
            for (col_idx, ch) in line.chars().enumerate() {
                if col_idx >= LED_COLS {
                    break;
                }
                if ch == '*' {
                    val |= 1 << col_idx;
                }
            }
            rows[row_idx] = val;
        }
        Glyph { rows }
    }

    /// Create an empty (all off) glyph.
    pub fn empty() -> Self {
        Glyph {
            rows: [0; LED_ROWS],
        }
    }

    /// Create a filled (all on) glyph.
    pub fn filled() -> Self {
        Glyph {
            rows: [0x1FF; LED_ROWS],
        }
    }

    /// Invert all pixels.
    pub fn invert(&self) -> Self {
        let mut rows = self.rows;
        for row in &mut rows {
            *row ^= 0x1FF; // 9 bits
        }
        Glyph { rows }
    }

    /// Encode the glyph as an 11-byte bitmap for the LED characteristic.
    pub fn to_bitmap(&self) -> [u8; LED_BITMAP_BYTES] {
        let mut buf = [0u8; LED_BITMAP_BYTES];
        let mut bit_pos = 0usize;

        for row in &self.rows {
            for col in 0..LED_COLS {
                if row & (1 << col) != 0 {
                    buf[bit_pos / 8] |= 1 << (bit_pos % 8);
                }
                bit_pos += 1;
            }
        }

        buf
    }

    /// Encode the full display payload (13 bytes: bitmap + brightness + timeout).
    pub fn to_display_bytes(&self, opts: &DisplayOptions) -> Vec<u8> {
        let mut bitmap = self.to_bitmap();

        // Apply fade flag (inverted: flag set = NO fade)
        if opts.transition == DisplayTransition::Immediate {
            bitmap[10] ^= LED_FADE_FLAG;
        }

        let brightness = (opts.brightness.clamp(0.0, 1.0) * 255.0) as u8;
        let timeout = (opts.timeout_ms.min(25500) / 100) as u8;

        let mut buf = Vec::with_capacity(13);
        buf.extend_from_slice(&bitmap);
        buf.push(brightness);
        buf.push(timeout);
        buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_glyph() {
        let g = Glyph::empty();
        let bitmap = g.to_bitmap();
        assert_eq!(bitmap, [0u8; 11]);
    }

    #[test]
    fn test_single_pixel_top_left() {
        let g = Glyph::from_str("*");
        let bitmap = g.to_bitmap();
        assert_eq!(bitmap[0], 0b00000001); // bit 0 set
        assert_eq!(bitmap[1], 0);
    }

    #[test]
    fn test_full_first_row() {
        let g = Glyph::from_str("*********");
        let bitmap = g.to_bitmap();
        // 9 bits: byte 0 = 0xFF (8 bits), byte 1 = 0x01 (1 bit)
        assert_eq!(bitmap[0], 0xFF);
        assert_eq!(bitmap[1], 0x01);
    }

    #[test]
    fn test_filled_glyph() {
        let g = Glyph::filled();
        let bitmap = g.to_bitmap();
        // 81 bits set across 11 bytes
        // byte 0..9 have patterns, byte 10 partial
        let total_bits: u32 = bitmap.iter().map(|b| b.count_ones()).sum();
        assert_eq!(total_bits, 81);
    }

    #[test]
    fn test_invert() {
        let g = Glyph::empty();
        let inv = g.invert();
        assert_eq!(inv, Glyph::filled());

        let g2 = Glyph::filled();
        let inv2 = g2.invert();
        assert_eq!(inv2, Glyph::empty());
    }

    #[test]
    fn test_display_bytes_length() {
        let g = Glyph::empty();
        let bytes = g.to_display_bytes(&DisplayOptions::default());
        assert_eq!(bytes.len(), 13);
    }

    #[test]
    fn test_display_bytes_brightness() {
        let g = Glyph::empty();
        let opts = DisplayOptions {
            brightness: 0.5,
            ..Default::default()
        };
        let bytes = g.to_display_bytes(&opts);
        assert_eq!(bytes[11], 127); // 0.5 * 255 ≈ 127
    }

    #[test]
    fn test_display_bytes_timeout() {
        let g = Glyph::empty();
        let opts = DisplayOptions {
            timeout_ms: 5000,
            ..Default::default()
        };
        let bytes = g.to_display_bytes(&opts);
        assert_eq!(bytes[12], 50); // 5000 / 100
    }

    #[test]
    fn test_display_bytes_fade_flag() {
        let g = Glyph::empty();

        let crossfade = g.to_display_bytes(&DisplayOptions {
            transition: DisplayTransition::CrossFade,
            ..Default::default()
        });
        assert_eq!(crossfade[10] & LED_FADE_FLAG, 0); // no flag = crossfade

        let immediate = g.to_display_bytes(&DisplayOptions {
            transition: DisplayTransition::Immediate,
            ..Default::default()
        });
        assert_ne!(immediate[10] & LED_FADE_FLAG, 0); // flag set = immediate
    }

    #[test]
    fn test_play_glyph() {
        let g = Glyph::from_str(
            "    *    \n\
                 **   \n\
                 ***  \n\
                 **** \n\
                 *****\n\
                 **** \n\
                 ***  \n\
                 **   \n\
                 *    ",
        );
        let bitmap = g.to_bitmap();
        let total_bits: u32 = bitmap.iter().map(|b| b.count_ones()).sum();
        assert_eq!(total_bits, 25); // play triangle has 25 pixels
    }
}
