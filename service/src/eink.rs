//! Continuous MSB-first white=1 wire pixels for 250×122 panels.
pub const WIDTH: usize = 250;
pub const HEIGHT: usize = 122;
pub const FRAME_BYTES: usize = (WIDTH * HEIGHT).div_ceil(8);
fn pack(mut white: impl FnMut(usize, usize) -> bool) -> Vec<u8> {
    let mut pixels = vec![0xff; FRAME_BYTES];
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            if !white(x, y) {
                let b = y * WIDTH + x;
                pixels[b / 8] &= !(0x80 >> (b % 8));
            }
        }
    }
    pixels
}
pub fn from_png(bytes: &[u8]) -> Option<Vec<u8>> {
    let image = image::load_from_memory(bytes).ok()?.to_luma8();
    if image.dimensions() != (WIDTH as u32, HEIGHT as u32) {
        return None;
    }
    Some(pack(|x, y| image.get_pixel(x as u32, y as u32)[0] >= 128))
}
/// Fit the existing LCD renderer with white side margins, preserving its aspect.
pub fn from_lcd(bytes: &[u8]) -> Vec<u8> {
    assert_eq!(bytes.len(), crate::protocol::LCD_FRAME_BYTES);
    let fitted_width = HEIGHT * 160 / 80;
    let left = (WIDTH - fitted_width) / 2;
    pack(|x, y| {
        if x < left || x >= left + fitted_width {
            return true;
        }
        let src = (y * 80 / HEIGHT * 160 + (x - left) * 160 / fitted_width) * 2;
        let v = u16::from_be_bytes([bytes[src], bytes[src + 1]]);
        let r = u32::from((v >> 11) & 31) * 255 / 31;
        let g = u32::from((v >> 5) & 63) * 255 / 63;
        let b = u32::from(v & 31) * 255 / 31;
        299 * r + 587 * g + 114 * b >= 128000
    })
}
pub fn draw_sub(pixels: &mut [u8]) {
    let glyphs = [[7, 4, 7, 1, 7], [5, 5, 5, 5, 7], [6, 5, 6, 5, 6]];
    for y in 108..119 {
        for x in 3..23 {
            let b = y * WIDTH + x;
            pixels[b / 8] |= 0x80 >> (b % 8);
        }
    }
    for (letter, glyph) in glyphs.iter().enumerate() {
        for (row, bits) in glyph.iter().enumerate() {
            for col in 0..3 {
                if bits & (1 << (2 - col)) != 0 {
                    let b = (110 + row) * WIDTH + 5 + letter * 5 + col;
                    pixels[b / 8] &= !(0x80 >> (b % 8));
                }
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn continuous_bits_white_padding_and_corners() {
        let pixels =
            pack(|x, y| ![(0, 0), (249, 0), (0, 121), (249, 121), (8, 1)].contains(&(x, y)));
        assert_eq!(pixels.len(), 3813);
        assert_eq!(pixels[0], 0x7f);
        assert_eq!(pixels[31] & 0x40, 0);
        assert_eq!(pixels[3812] & 0x10, 0);
        assert_eq!(pixels[3812] & 0x0f, 0x0f);
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let b = y * WIDTH + x;
                assert_eq!(
                    pixels[b / 8] & (0x80 >> (b % 8)) != 0,
                    ![(0, 0), (249, 0), (0, 121), (249, 121), (8, 1)].contains(&(x, y))
                );
            }
        }
    }
    #[test]
    fn lcd_fit_and_monochrome_threshold() {
        let white = from_lcd(&vec![0xff; crate::protocol::LCD_FRAME_BYTES]);
        assert!(white.iter().all(|v| *v == 255));
        let black = from_lcd(&vec![0; crate::protocol::LCD_FRAME_BYTES]);
        let bit = |x: usize, y: usize| {
            black[(y * WIDTH + x) / 8] & (0x80u8 >> ((y * WIDTH + x) % 8)) != 0
        };
        assert!(bit(0, 0) && bit(249, 121));
        assert!(!bit(3, 0) && !bit(246, 121));
    }
}
