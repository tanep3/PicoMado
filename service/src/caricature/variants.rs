//! Replaceable display variants. The immutable archive remains the source of truth.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use image::imageops::FilterType;

use super::external;
use super::hash::sha256_hex;
use super::store::ArtworkStore;
use super::{Artwork, CaricatureError, HEIGHT_EINK, HEIGHT_LCD, WIDTH_EINK, WIDTH_LCD};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelFormat {
    Rgb565Be,
    EinkPng1,
}

#[derive(Clone, Copy, Debug)]
pub struct VariantSpec {
    pub width: u32,
    pub height: u32,
    pub format: PixelFormat,
    pub dpid_lambda: f64,
    pub threshold: u8,
}

impl VariantSpec {
    pub fn lcd(art: &Artwork) -> Self {
        Self {
            width: WIDTH_LCD as u32,
            height: HEIGHT_LCD as u32,
            format: PixelFormat::Rgb565Be,
            dpid_lambda: art.metadata.settings.dpid_lambda,
            threshold: art.metadata.settings.threshold,
        }
    }

    pub fn eink(art: &Artwork) -> Self {
        Self {
            width: WIDTH_EINK,
            height: HEIGHT_EINK,
            format: PixelFormat::EinkPng1,
            dpid_lambda: art.metadata.settings.dpid_lambda,
            threshold: art.metadata.settings.threshold,
        }
    }

    fn validate(self) -> Result<(), CaricatureError> {
        if self.width == 0
            || self.height == 0
            || self.width > 2048
            || self.height > 2048
            || !self.dpid_lambda.is_finite()
            || !(0.0..=1.0).contains(&self.dpid_lambda)
            || !(1..=254).contains(&self.threshold)
        {
            return Err(CaricatureError::InvalidRequest);
        }
        Ok(())
    }
}

pub struct CachedVariant {
    pub bytes: Vec<u8>,
    pub hit: bool,
}

pub struct VariantCache {
    root: PathBuf,
}

impl VariantCache {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn key(&self, art: &Artwork, spec: VariantSpec) -> String {
        let source = art.original_sha256.clone().unwrap_or_else(|| {
            format!(
                "legacy:{}:{}",
                sha256_hex(&art.eink_png),
                sha256_hex(&art.lcd_rgb565be)
            )
        });
        let descriptor = format!(
            "caricature-variant-v1|{}|{}|{}|{}|{:?}|{:016x}|{}|nearest-white-fit|dpid",
            art.id,
            source,
            spec.width,
            spec.height,
            spec.format,
            spec.dpid_lambda.to_bits(),
            spec.threshold
        );
        sha256_hex(descriptor.as_bytes())
    }

    pub fn get_or_render(
        &self,
        store: &ArtworkStore,
        art: &Artwork,
        spec: VariantSpec,
        python: &Path,
    ) -> Result<CachedVariant, CaricatureError> {
        spec.validate()?;
        if art.id.is_empty()
            || art.id.len() > 100
            || !art
                .id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err(CaricatureError::InvalidSource);
        }
        let native = spec.dpid_lambda == art.metadata.settings.dpid_lambda
            && spec.threshold == art.metadata.settings.threshold;
        let archived = match spec.format {
            PixelFormat::Rgb565Be
                if native && spec.width == WIDTH_LCD as u32 && spec.height == HEIGHT_LCD as u32 =>
            {
                Some(art.lcd_rgb565be.clone())
            }
            PixelFormat::EinkPng1
                if native && spec.width == WIDTH_EINK && spec.height == HEIGHT_EINK =>
            {
                Some(art.eink_png.clone())
            }
            _ => None,
        };
        if let Some(bytes) = archived {
            if !valid_variant(&bytes, spec) {
                return Err(CaricatureError::InvalidSource);
            }
            // The archive JSON already stores this exact display cache.
            return Ok(CachedVariant { bytes, hit: true });
        }
        let path = self
            .root
            .join(format!("{}-{}.bin", art.id, self.key(art, spec)));
        if let Ok(data) = fs::read(&path) {
            if data.len() >= 68
                && &data[..4] == b"CV01"
                && data[4..68] == *sha256_hex(&data[68..]).as_bytes()
                && valid_variant(&data[68..], spec)
            {
                return Ok(CachedVariant {
                    bytes: data[68..].to_vec(),
                    hit: true,
                });
            }
        }
        let bytes = match spec.format {
            PixelFormat::Rgb565Be => {
                let source = store
                    .original(art)?
                    .or_else(|| art.original_sha256.is_none().then(|| art.eink_png.clone()));
                render_rgb565(
                    &source.ok_or(CaricatureError::InvalidSource)?,
                    spec.width,
                    spec.height,
                )?
            }
            PixelFormat::EinkPng1 => {
                let source = store
                    .original(art)?
                    .or_else(|| art.original_sha256.is_none().then(|| art.eink_png.clone()));
                external::render_eink_variant(
                    &source.ok_or(CaricatureError::InvalidSource)?,
                    spec.width,
                    spec.height,
                    spec.dpid_lambda,
                    spec.threshold,
                    python,
                    &self.root,
                )?
            }
        };
        if !valid_variant(&bytes, spec) {
            return Err(CaricatureError::InvalidSource);
        }
        // Cache is disposable. A read-only/full cache must not block display.
        let _ = write_cache(&path, &bytes);
        Ok(CachedVariant { bytes, hit: false })
    }
}

fn valid_variant(bytes: &[u8], spec: VariantSpec) -> bool {
    match spec.format {
        PixelFormat::Rgb565Be => bytes.len() == (spec.width as usize * spec.height as usize * 2),
        PixelFormat::EinkPng1 => {
            bytes.len() >= 33
                && bytes.starts_with(b"\x89PNG\r\n\x1a\n")
                && &bytes[12..16] == b"IHDR"
                && u32::from_be_bytes(bytes[16..20].try_into().unwrap()) == spec.width
                && u32::from_be_bytes(bytes[20..24].try_into().unwrap()) == spec.height
                && bytes[24..29] == [1, 0, 0, 0, 0]
                && image::load_from_memory(bytes).is_ok()
        }
    }
}

pub fn render_rgb565(original: &[u8], width: u32, height: u32) -> Result<Vec<u8>, CaricatureError> {
    if width == 0 || height == 0 || width > 2048 || height > 2048 {
        return Err(CaricatureError::InvalidRequest);
    }
    let source = image::load_from_memory(original).map_err(|_| CaricatureError::InvalidSource)?;
    let fitted = source.resize(width, height, FilterType::Nearest).to_rgb8();
    let mut output = vec![0xff; width as usize * height as usize * 2];
    let x0 = (width - fitted.width()) / 2;
    let y0 = (height - fitted.height()) / 2;
    for y in 0..fitted.height() {
        for x in 0..fitted.width() {
            let p = fitted.get_pixel(x, y);
            let color = (((p[0] as u16) & 0xf8) << 8)
                | (((p[1] as u16) & 0xfc) << 3)
                | ((p[2] as u16) >> 3);
            let i = (((y + y0) * width + x + x0) * 2) as usize;
            output[i..i + 2].copy_from_slice(&color.to_be_bytes());
        }
    }
    Ok(output)
}

fn write_cache(path: &Path, bytes: &[u8]) -> Result<(), std::io::Error> {
    fs::create_dir_all(path.parent().ok_or(std::io::Error::other("cache path"))?)?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temp = path.with_extension(format!("tmp-{}-{stamp}", std::process::id()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(b"CV01")?;
        file.write_all(sha256_hex(bytes).as_bytes())?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::caricature::{ArtworkMetadata, GenerationInput, GenerationSettings};
    use image::{DynamicImage, ImageFormat, Rgb, RgbImage};
    use std::io::Cursor;

    fn sample() -> (Artwork, Vec<u8>) {
        let mut pixels = RgbImage::new(320, 160);
        for (x, _, pixel) in pixels.enumerate_pixels_mut() {
            *pixel = if x < 160 {
                Rgb([240, 20, 20])
            } else {
                Rgb([20, 80, 230])
            };
        }
        let mut writer = Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(pixels)
            .write_to(&mut writer, ImageFormat::Png)
            .unwrap();
        let original = writer.into_inner();
        let art = Artwork {
            id: "color-1".into(),
            metadata: ArtworkMetadata {
                job_id: "color".into(),
                slot: 0,
                created_at_unix: 0,
                input: GenerationInput::News,
                source_urls: vec![],
                news_summary: None,
                settings: GenerationSettings::default(),
            },
            original_sha256: Some(sha256_hex(&original)),
            original_codec: None,
            eink_png: crate::caricature::test_canonical_png(),
            lcd_rgb565be: render_rgb565(&original, 160, 80).unwrap(),
        };
        (art, original)
    }

    #[test]
    fn color_source_and_cache_profiles() {
        let dir = tempfile::tempdir().unwrap();
        let store = ArtworkStore::new(dir.path().join("artwork"));
        let cache = VariantCache::new(dir.path().join("cache"));
        let (art, original) = sample();
        store
            .publish_with_original(art.clone(), &original, 0)
            .unwrap();
        let spec = VariantSpec::lcd(&art);
        let native = cache
            .get_or_render(&store, &art, spec, Path::new("unused"))
            .unwrap();
        assert!(native.hit);
        assert!(!cache.root.exists());
        let smaller = VariantSpec {
            width: 80,
            height: 40,
            ..spec
        };
        let started = std::time::Instant::now();
        let first = cache
            .get_or_render(&store, &art, smaller, Path::new("unused"))
            .unwrap();
        let miss_elapsed = started.elapsed();
        assert!(!first.hit);
        let started = std::time::Instant::now();
        let second = cache
            .get_or_render(&store, &art, smaller, Path::new("unused"))
            .unwrap();
        let hit_elapsed = started.elapsed();
        assert!(second.hit);
        eprintln!("320x160 synthetic 80x40 LCD variant miss={miss_elapsed:?}, hit={hit_elapsed:?}");
        assert_eq!(first.bytes, second.bytes);
        let colors: std::collections::BTreeSet<_> = native
            .bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_be_bytes(*pair))
            .collect();
        assert!(colors.contains(&0xf0a2));
        assert!(colors.contains(&0x129c));
        let cached_path = cache
            .root
            .join(format!("{}-{}.bin", art.id, cache.key(&art, smaller)));
        fs::write(&cached_path, b"corrupt").unwrap();
        assert!(
            !cache
                .get_or_render(&store, &art, smaller, Path::new("unused"))
                .unwrap()
                .hit
        );
        assert!(
            cache
                .get_or_render(&store, &art, smaller, Path::new("unused"))
                .unwrap()
                .hit
        );
        assert_eq!(second.bytes.len(), 80 * 40 * 2);
        assert_ne!(cache.key(&art, spec), cache.key(&art, smaller));
        let threshold = VariantSpec {
            threshold: spec.threshold + 1,
            ..spec
        };
        assert_ne!(cache.key(&art, spec), cache.key(&art, threshold));
        assert_ne!(
            cache.key(&art, spec),
            cache.key(&art, VariantSpec::eink(&art))
        );
        let mut new_revision = art.clone();
        new_revision.original_sha256 = Some("a".repeat(64));
        assert_ne!(cache.key(&art, spec), cache.key(&new_revision, spec));
    }

    #[test]
    fn old_history_and_missing_new_original() {
        let dir = tempfile::tempdir().unwrap();
        let store = ArtworkStore::new(dir.path().join("artwork"));
        let cache = VariantCache::new(dir.path().join("cache"));
        let (mut art, original) = sample();
        art.original_sha256 = None;
        art.lcd_rgb565be = render_rgb565(&art.eink_png, 160, 80).unwrap();
        store.publish(art.clone(), 0).unwrap();
        let small = VariantSpec {
            width: 80,
            height: 40,
            ..VariantSpec::lcd(&art)
        };
        assert!(
            !cache
                .get_or_render(&store, &art, small, Path::new("unused"))
                .unwrap()
                .hit
        );
        assert!(
            cache
                .get_or_render(&store, &art, small, Path::new("unused"))
                .unwrap()
                .hit
        );

        let (mut fresh, _) = sample();
        fresh.id = "fresh-1".into();
        fresh.metadata.job_id = "fresh".into();
        let webp = crate::caricature::originals::half_webp_quality90(&original).unwrap();
        fresh.original_sha256 = Some(sha256_hex(&webp));
        fresh.original_codec = Some(crate::caricature::OriginalCodec::HalfWebpQ90);
        store
            .publish_with_original(fresh.clone(), &webp, 86_400)
            .unwrap();
        fs::remove_file(dir.path().join("artwork/archive/fresh-1.original.webp")).unwrap();
        assert!(cache
            .get_or_render(
                &store,
                &fresh,
                VariantSpec::lcd(&fresh),
                Path::new("unused")
            )
            .is_ok());
        assert!(cache
            .get_or_render(&store, &fresh, small, Path::new("unused"))
            .is_err());
    }
}
