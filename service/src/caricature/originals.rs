//! Bounded half-size, lossy WebP originals. LCD and E-ink still render from
//! the generated PNG before this archival conversion.

use std::ffi::c_void;

use image::{imageops::FilterType, GenericImageView};

use super::CaricatureError;

const QUALITY: f32 = 90.0;

#[link(name = "webp")]
unsafe extern "C" {
    fn WebPEncodeRGB(
        rgb: *const u8,
        width: i32,
        height: i32,
        stride: i32,
        quality_factor: f32,
        output: *mut *mut u8,
    ) -> usize;
    fn WebPFree(pointer: *mut c_void);
}

pub fn half_webp_quality90(png: &[u8]) -> Result<Vec<u8>, CaricatureError> {
    if png.is_empty() || png.len() > 20_000_000 {
        return Err(CaricatureError::InvalidSource);
    }
    let decoded = image::load_from_memory(png).map_err(|_| CaricatureError::InvalidSource)?;
    let (width, height) = decoded.dimensions();
    if width == 0
        || height == 0
        || width > 8192
        || height > 8192
        || width as u64 * height as u64 > 20_000_000
    {
        return Err(CaricatureError::InvalidSource);
    }
    // Odd dimensions round up, so no border pixel is silently discarded.
    let target_width = width.div_ceil(2);
    let target_height = height.div_ceil(2);
    let resized = decoded
        .resize_exact(target_width, target_height, FilterType::Lanczos3)
        .to_rgba8();
    let mut rgb = Vec::with_capacity((target_width * target_height * 3) as usize);
    for pixel in resized.pixels() {
        let alpha = pixel[3] as u32;
        for channel in &pixel.0[..3] {
            rgb.push(((*channel as u32 * alpha + 255 * (255 - alpha) + 127) / 255) as u8);
        }
    }
    let bytes = encode_rgb(&rgb, target_width, target_height, QUALITY)?;
    validate_half_webp(&bytes, Some((target_width, target_height)))?;
    Ok(bytes)
}

fn encode_rgb(
    rgb: &[u8],
    width: u32,
    height: u32,
    quality: f32,
) -> Result<Vec<u8>, CaricatureError> {
    let mut pointer = std::ptr::null_mut();
    // SAFETY: rgb has exactly width*height*3 initialized bytes,
    // both dimensions/stride fit i32, and libwebp owns the returned pointer.
    let length = unsafe {
        WebPEncodeRGB(
            rgb.as_ptr(),
            width as i32,
            height as i32,
            (width * 3) as i32,
            quality,
            &mut pointer,
        )
    };
    if length == 0 || pointer.is_null() || length > 20_000_000 {
        if !pointer.is_null() {
            unsafe { WebPFree(pointer.cast()) };
        }
        return Err(CaricatureError::InvalidSource);
    }
    let bytes = unsafe { std::slice::from_raw_parts(pointer, length).to_vec() };
    unsafe { WebPFree(pointer.cast()) };
    Ok(bytes)
}

pub fn validate_half_webp(
    bytes: &[u8],
    expected_dimensions: Option<(u32, u32)>,
) -> Result<(), CaricatureError> {
    if bytes.len() < 20
        || bytes.len() > 20_000_000
        || &bytes[..4] != b"RIFF"
        || &bytes[8..16] != b"WEBPVP8 "
        || u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize + 8 != bytes.len()
    {
        return Err(CaricatureError::InvalidSource);
    }
    let image = image::load_from_memory(bytes).map_err(|_| CaricatureError::InvalidSource)?;
    if expected_dimensions.is_some_and(|size| image.dimensions() != size) {
        return Err(CaricatureError::InvalidSource);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, ImageFormat, Rgb, RgbImage};
    use std::io::Cursor;

    #[test]
    fn half_size_quality90_is_lossy_vp8_and_keeps_color() {
        let mut pixels = RgbImage::new(101, 51);
        for (x, y, pixel) in pixels.enumerate_pixels_mut() {
            *pixel = Rgb([(x * 2) as u8, (y * 4) as u8, ((x + y) * 2) as u8]);
        }
        let mut output = Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(pixels.clone())
            .write_to(&mut output, ImageFormat::Png)
            .unwrap();
        let source = output.into_inner();
        let webp = half_webp_quality90(&source).unwrap();
        let resized = image::load_from_memory(&source)
            .unwrap()
            .resize_exact(51, 26, FilterType::Lanczos3)
            .to_rgb8();
        assert_eq!(webp, encode_rgb(resized.as_raw(), 51, 26, 90.0).unwrap());
        assert_ne!(webp, encode_rgb(resized.as_raw(), 51, 26, 50.0).unwrap());
        assert_eq!(&webp[12..16], b"VP8 "); // VP8L would be lossless.
        let decoded = image::load_from_memory(&webp).unwrap();
        assert_eq!(decoded.dimensions(), (51, 26));
        let rgb = decoded.to_rgb8();
        assert!(rgb.pixels().any(|p| p[0] != p[1] || p[1] != p[2]));
        assert!(validate_half_webp(&webp, Some((50, 25))).is_err());
    }
}
