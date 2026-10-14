//! Native-size Japanese bitmap text. The charset and measured glyph metrics
//! are generated together; LCD rendering never rescales a glyph tile.

use serde::Deserialize;
use std::sync::LazyLock;

const PIXELS: &[u8] = include_bytes!("../assets/weather-glyphs.bin");
const METRICS: &str = include_str!("../assets/weather-glyph-metrics.json");

#[derive(Deserialize)]
struct Glyph {
    offset: usize,
    width: usize,
    height: usize,
    x: i32,
    y: i32,
    advance: usize,
}

#[derive(Deserialize)]
struct Font {
    sizes: Vec<usize>,
    chars: String,
    glyphs: Vec<Glyph>,
}

static FONT: LazyLock<Font> =
    LazyLock::new(|| serde_json::from_str(METRICS).expect("LCD font metrics"));

fn glyph(ch: char, size: usize) -> &'static Glyph {
    let size_index = FONT
        .sizes
        .iter()
        .position(|v| *v == size)
        .expect("native LCD font size");
    let char_index = FONT.chars.chars().position(|v| v == ch).unwrap_or_else(|| {
        FONT.chars
            .chars()
            .position(|v| v == '?')
            .expect("fallback glyph")
    });
    &FONT.glyphs[size_index * FONT.chars.chars().count() + char_index]
}

pub(crate) fn width(text: &str, size: usize) -> usize {
    text.chars().map(|ch| glyph(ch, size).advance).sum()
}

/// Keep labels at a readable size and make omission explicit, without cutting
/// a multibyte character or drawing a partial glyph at the screen edge.
pub(crate) fn fit(text: &str, size: usize, max_width: usize) -> String {
    if width(text, size) <= max_width {
        return text.into();
    }
    let ellipsis = width("…", size);
    if ellipsis > max_width {
        return String::new();
    }
    let mut result = String::new();
    let mut used = 0;
    for ch in text.chars() {
        let advance = glyph(ch, size).advance;
        if used + advance + ellipsis > max_width {
            break;
        }
        result.push(ch);
        used += advance;
    }
    result.push('…');
    result
}

pub(crate) fn draw(
    pixels: &mut [u16],
    dims: (usize, usize),
    origin: (i32, i32),
    text: &str,
    size: usize,
    color: u16,
) {
    let (w, h) = dims;
    let (mut cursor, y) = origin;
    for ch in text.chars() {
        let g = glyph(ch, size);
        for row in 0..g.height {
            for col in 0..g.width {
                let px = cursor + g.x + col as i32;
                let py = y + g.y + row as i32;
                if px >= 0
                    && py >= 0
                    && px < w as i32
                    && py < h as i32
                    && PIXELS[g.offset + row * g.width + col] > 96
                {
                    pixels[py as usize * w + px as usize] = color;
                }
            }
        }
        cursor += g.advance as i32;
    }
}

/// Shared pixel/text primitive; the caller owns the plugin/status wording.
pub(crate) fn status_frame(title: &str, status: &str) -> Vec<u8> {
    let mut pixels = vec![0x0841; 160 * 80];
    let title = fit(title, 16, 144);
    let status = fit(status, 14, 144);
    draw(&mut pixels, (160, 80), (8, 10), &title, 16, 0x07ff);
    draw(&mut pixels, (160, 80), (8, 44), &status, 14, 0xffff);
    pixels.into_iter().flat_map(u16::to_be_bytes).collect()
}

/// Kernel-owned all-OFF screen; center the actual glyph bounds.
pub(crate) fn logo_frame() -> Vec<u8> {
    let text = "PICO窓";
    let size = 22;
    let top = text.chars().map(|ch| glyph(ch, size).y).min().unwrap();
    let bottom = text
        .chars()
        .map(|ch| {
            let g = glyph(ch, size);
            g.y + g.height as i32
        })
        .max()
        .unwrap();
    let mut pixels = vec![0xffff; 160 * 80];
    draw(
        &mut pixels,
        (160, 80),
        (
            ((160 - width(text, size)) / 2) as i32,
            (80 - (bottom - top)) / 2 - top,
        ),
        text,
        size,
        0,
    );
    pixels.into_iter().flat_map(u16::to_be_bytes).collect()
}

#[cfg(test)]
pub(crate) fn assert_text_fits(
    text: &str,
    size: usize,
    origin: (usize, usize),
    dims: (usize, usize),
) {
    let mut cursor = origin.0;
    for ch in text.chars() {
        let g = glyph(ch, size);
        assert!(cursor as i32 + g.x >= 0);
        assert!(origin.1 as i32 + g.y >= 0);
        assert!(
            cursor + g.x as usize + g.width <= dims.0,
            "{text}: horizontal clipping"
        );
        assert!(
            origin.1 + g.y as usize + g.height <= dims.1,
            "{text}: vertical clipping"
        );
        cursor += g.advance;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_off_logo_is_centered_black_on_white_with_real_window_glyph() {
        assert!(FONT.chars.contains('窓'));
        let pixels = logo_frame();
        let black: Vec<_> = pixels
            .as_chunks::<2>()
            .0
            .iter()
            .enumerate()
            .filter(|(_, p)| **p == [0, 0])
            .map(|(i, _)| (i % 160, i / 160))
            .collect();
        assert!(!black.is_empty());
        let left = black.iter().map(|p| p.0).min().unwrap();
        let right = black.iter().map(|p| p.0).max().unwrap();
        let top = black.iter().map(|p| p.1).min().unwrap();
        let bottom = black.iter().map(|p| p.1).max().unwrap();
        assert!((left + right).abs_diff(159) <= 4);
        assert!((top + bottom).abs_diff(79) <= 2);
        assert!(pixels.iter().all(|p| *p == 0 || *p == 255));
    }

    #[test]
    fn atlas_charset_metrics_and_japanese_face_are_consistent() {
        assert_eq!(FONT.chars, include_str!("../assets/weather-chars.txt"));
        let chars = FONT.chars.chars().count();
        assert_eq!(
            FONT.chars
                .chars()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            chars
        );
        assert_eq!(FONT.sizes, [12, 14, 16, 20, 22]);
        assert_eq!(FONT.glyphs.len(), FONT.sizes.len() * chars);
        let mut offset = 0;
        for (index, g) in FONT.glyphs.iter().enumerate() {
            assert_eq!(g.offset, offset);
            assert!(g.advance > 0);
            let end = offset + g.width * g.height;
            assert!(end <= PIXELS.len());
            if !FONT
                .chars
                .chars()
                .nth(index % chars)
                .unwrap()
                .is_whitespace()
            {
                assert!(PIXELS[offset..end].iter().any(|alpha| *alpha > 96));
            }
            offset += g.width * g.height;
        }
        assert_eq!(offset, PIXELS.len());
        let metadata: serde_json::Value = serde_json::from_str(METRICS).unwrap();
        assert_eq!(metadata["font"]["family"], "Noto Sans CJK JP");
        assert_eq!(metadata["font"]["style"], "Bold");
        assert_eq!(metadata["font"]["collection_index"], 0);
        for location in crate::weather::LOCATIONS.iter() {
            for label in [
                &location.label,
                &location.office_label,
                &location.area_label,
                &location.point_label,
            ] {
                for ch in label.chars() {
                    assert!(FONT.chars.contains(ch), "missing Japanese glyph: {ch}");
                }
            }
        }
    }

    #[test]
    fn fitted_labels_and_native_glyphs_stay_inside_measured_bounds() {
        for &size in &FONT.sizes {
            let text = fit("予報:鹿児島県（奄美地方を除く）", size, 152);
            assert!(text.ends_with('…'));
            assert!(width(&text, size) <= 152);
            let mut pixels = vec![0; 160 * 32];
            draw(&mut pixels, (160, 32), (4, 0), &text, size, 1);
            assert!(pixels.contains(&1));
            for (index, _) in pixels.iter().enumerate().filter(|(_, v)| **v == 1) {
                assert!(index % 160 >= 4 && index % 160 < 4 + width(&text, size));
            }
        }
    }
}
