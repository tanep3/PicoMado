//! Reproducible previews without opening the application DB or network.
use image::{Rgb, RgbImage};
use picomado_core::clock::{render_eink, render_lcd};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = std::path::PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "/tmp/picomado-clock-previews".into()),
    );
    std::fs::create_dir_all(&dir)?;
    for (name, y, m, d, h, min) in [
        ("sample", 2026, 10, 18, 10, 8),
        ("2330", 2026, 10, 18, 23, 30),
        ("year-end", 2026, 12, 31, 23, 59),
        ("new-year", 2027, 1, 1, 0, 0),
    ] {
        let now = time::Date::from_calendar_date(y, time::Month::try_from(m)?, d)?
            .with_hms(h, min, 0)?
            .assume_offset(time::UtcOffset::from_hms(9, 0, 0)?)
            .unix_timestamp();
        let lcd = render_lcd(now);
        let mono = render_eink(now);
        let lcd_image = RgbImage::from_fn(160, 80, |x, y| {
            let i = (y * 160 + x) as usize * 2;
            let c = u16::from_be_bytes([lcd[i], lcd[i + 1]]);
            Rgb([
                (((c >> 11) & 31) * 255 / 31) as u8,
                (((c >> 5) & 63) * 255 / 63) as u8,
                ((c & 31) * 255 / 31) as u8,
            ])
        });
        let eink_image = RgbImage::from_fn(250, 122, |x, y| {
            let b = (y * 250 + x) as usize;
            let c = if mono[b / 8] & (0x80 >> (b % 8)) != 0 {
                255
            } else {
                0
            };
            Rgb([c, c, c])
        });
        lcd_image.save(dir.join(format!("clock-{name}-lcd.png")))?;
        eink_image.save(dir.join(format!("clock-{name}-eink.png")))?;
        let mut preview = RgbImage::from_pixel(1000, 540, Rgb([48, 48, 48]));
        let lcd_large =
            image::imageops::resize(&lcd_image, 640, 320, image::imageops::FilterType::Nearest);
        let eink_large =
            image::imageops::resize(&eink_image, 1000, 488, image::imageops::FilterType::Nearest);
        image::imageops::replace(&mut preview, &lcd_large, 180, 0);
        preview.save(dir.join(format!("clock-{name}-lcd-enlarged.png")))?;
        eink_large.save(dir.join(format!("clock-{name}-eink-enlarged.png")))?;
    }
    println!("{}", dir.display());
    Ok(())
}
