//! Local-music-only Noto Sans CJK JP bitmap text. Weather assets remain untouched.
const INDEX: &[u8] = include_bytes!("../assets/music-glyph-index.bin");
const PIXELS: &[u8] = include_bytes!("../assets/music-glyphs.bin");
fn record(ch: char) -> Option<usize> {
    let mut lo = 0;
    let mut hi = INDEX.len() / 24;
    while lo < hi {
        let m = (lo + hi) / 2;
        let cp = u32::from_le_bytes(INDEX[m * 24..m * 24 + 4].try_into().unwrap());
        match cp.cmp(&(ch as u32)) {
            std::cmp::Ordering::Equal => return Some(m),
            std::cmp::Ordering::Less => lo = m + 1,
            _ => hi = m,
        }
    }
    None
}
fn glyph(ch: char, size: usize) -> &'static [u8] {
    let i = record(ch).unwrap_or_else(|| record('�').unwrap());
    assert!(size == 14 || size == 16);
    &INDEX[i * 24 + 4 + if size == 14 { 0 } else { 10 }..][..10]
}
fn width(s: &str, size: usize) -> usize {
    s.chars().map(|c| glyph(c, size)[4] as usize).sum()
}
fn fit(s: &str, size: usize, max: usize) -> String {
    if width(s, size) <= max {
        return s.into();
    }
    let mut out = String::new();
    let mut used = width("…", size);
    for c in s.chars() {
        let n = glyph(c, size)[4] as usize;
        if used + n > max {
            break;
        }
        out.push(c);
        used += n;
    }
    out.push('…');
    out
}
fn draw(
    pixels: &mut [u16],
    dims: (usize, usize),
    origin: (i32, i32),
    text: &str,
    size: usize,
    color: u16,
) {
    let (w, h) = dims;
    let (mut x, y) = origin;
    for c in text.chars() {
        let g = glyph(c, size);
        let gw = g[0] as usize;
        let gh = g[1] as usize;
        let offset = u32::from_le_bytes(g[6..10].try_into().unwrap()) as usize;
        for row in 0..gh {
            for col in 0..gw {
                let bit = row * gw + col;
                let px = x + g[2] as i8 as i32 + col as i32;
                let py = y + g[3] as i8 as i32 + row as i32;
                if px >= 0
                    && py >= 0
                    && px < w as i32
                    && py < h as i32
                    && PIXELS[offset + bit / 8] & (0x80 >> (bit % 8)) != 0
                {
                    pixels[py as usize * w + px as usize] = color;
                }
            }
        }
        x += g[4] as i32;
    }
}
pub(crate) fn lcd(title: &str, name: &str) -> Vec<u8> {
    let mut pixels = vec![0x0841; 160 * 80];
    draw(
        &mut pixels,
        (160, 80),
        (8, 8),
        &fit(title, 14, 144),
        14,
        0x07ff,
    );
    draw(
        &mut pixels,
        (160, 80),
        (8, 40),
        &fit(name, 16, 144),
        16,
        0xffff,
    );
    pixels.into_iter().flat_map(u16::to_be_bytes).collect()
}
pub(crate) fn eink(title: &str, name: &str) -> Vec<u8> {
    let mut pixels = vec![0xffff; 250 * 122];
    draw(
        &mut pixels,
        (250, 122),
        (8, 12),
        &fit(title, 16, 234),
        16,
        0,
    );
    draw(&mut pixels, (250, 122), (8, 52), &fit(name, 16, 234), 16, 0);
    let mut packed = vec![0xff; (250usize * 122).div_ceil(8)];
    for (i, p) in pixels.into_iter().enumerate() {
        if p == 0 {
            packed[i / 8] &= !(0x80 >> (i % 8));
        }
    }
    packed
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn filename_charset_and_bounded_glyphs() {
        for cp in 32..127 {
            assert!(record(char::from_u32(cp).unwrap()).is_some());
        }
        for ch in
            "夜に駆ける 日本語の曲名 あいうえお アイウエオ ｱｲｳｴｵ Track.name-01_(live).mp3".chars()
        {
            assert!(record(ch).is_some(), "{ch}");
        }
        for i in 0..INDEX.len() / 24 {
            for size in [14, 16] {
                let cp = u32::from_le_bytes(INDEX[i * 24..i * 24 + 4].try_into().unwrap());
                let g = glyph(char::from_u32(cp).unwrap(), size);
                let end = u32::from_le_bytes(g[6..10].try_into().unwrap()) as usize
                    + (g[0] as usize * g[1] as usize).div_ceil(8);
                assert!(end <= PIXELS.len());
            }
        }
    }
    #[test]
    fn native_lcd_and_eink_not_question_marks() {
        assert_eq!(lcd("MUSIC REQUESTED", "夜に駆ける.mp3").len(), 25600);
        assert_eq!(
            eink("MUSIC REQUESTED", "夜に駆ける.mp3").len(),
            crate::eink::FRAME_BYTES
        );
        assert_ne!(lcd("", "abc.mp3"), lcd("", "???????"));
        assert_ne!(eink("", "日本語"), eink("", "???"));
        assert!(width(&fit("日本語の長い曲名とファイル名.mp3", 16, 144), 16) <= 144);
    }
}
