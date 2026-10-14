//! A transient indicator composed into delivered LCD pixels, never source art.

const WIDTH: usize = 160;
const HEIGHT: usize = 80;
const LEFT: usize = 3;
const TOP: usize = 69;
const BOX_WIDTH: usize = 18;
const BOX_HEIGHT: usize = 9;

// Three 3×5 glyphs; bit 2 is the leftmost pixel.
const SUB: [[u8; 5]; 3] = [
    [0b111, 0b100, 0b111, 0b001, 0b111],
    [0b101, 0b101, 0b101, 0b101, 0b111],
    [0b110, 0b101, 0b110, 0b101, 0b110],
];

/// Draw a white box with black "SUB" at the lower left of a 160×80 RGB565BE
/// frame. The caller owns this delivery buffer; archived pixels stay intact.
pub(crate) fn draw_sub_rgb565be(pixels: &mut [u8]) {
    if pixels.len() != WIDTH * HEIGHT * 2 {
        return;
    }
    for y in TOP..TOP + BOX_HEIGHT {
        for x in LEFT..LEFT + BOX_WIDTH {
            let offset = (y * WIDTH + x) * 2;
            pixels[offset..offset + 2].copy_from_slice(&0xffffu16.to_be_bytes());
        }
    }
    for (letter, glyph) in SUB.iter().enumerate() {
        for (row, bits) in glyph.iter().enumerate() {
            for col in 0..3 {
                if bits & (1 << (2 - col)) != 0 {
                    let x = LEFT + 2 + letter * 4 + col;
                    let y = TOP + 2 + row;
                    let offset = (y * WIDTH + x) * 2;
                    pixels[offset..offset + 2].copy_from_slice(&0u16.to_be_bytes());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn badge_is_small_legible_and_confined_to_lower_left() {
        let mut pixels = vec![0x42; WIDTH * HEIGHT * 2];
        draw_sub_rgb565be(&mut pixels);
        let color = |x: usize, y: usize| -> [u8; 2] {
            pixels[(y * WIDTH + x) * 2..(y * WIDTH + x) * 2 + 2]
                .try_into()
                .unwrap()
        };
        assert_eq!(color(LEFT, TOP), [0xff, 0xff]);
        assert_eq!(color(LEFT + 2, TOP + 2), [0, 0]);
        assert_eq!(color(LEFT + 6, TOP + 2), [0, 0]);
        assert_eq!(color(LEFT + 10, TOP + 2), [0, 0]);
        assert_eq!(color(LEFT - 1, TOP), [0x42, 0x42]);
        assert_eq!(color(LEFT + BOX_WIDTH, TOP), [0x42, 0x42]);
        assert_eq!(color(LEFT, TOP - 1), [0x42, 0x42]);
        assert_eq!(color(LEFT, TOP + BOX_HEIGHT), [0x42, 0x42]);
    }
}
