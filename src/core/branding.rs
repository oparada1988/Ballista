use crate::core::animation::{apply_hardware_transform, encode_hardware_image};
use crate::models::{DeviceKind, KeyImageFormat};
use image::{imageops::FilterType, ImageBuffer, Rgb, RgbImage, RgbaImage};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

const LOGO_2X2_PNG: &[u8] = include_bytes!("../../resources/branding/logo-2x2.png");
const TOUCHSTRIP_PNG: &[u8] = include_bytes!("../../resources/branding/touchstrip.png");

static DECODED_LOGO_RGB: OnceLock<RgbImage> = OnceLock::new();
static DECODED_TOUCHSTRIP_RGB: OnceLock<RgbImage> = OnceLock::new();
static RENDER_CACHE: OnceLock<Mutex<HashMap<DeviceKind, BrandingFrames>>> = OnceLock::new();

/// Pre-encoded native hardware frames for displaying the Ballista branding
/// (center 2x2 logo + black surround keys + touch strip / info bar) on a Stream Deck model.
#[derive(Clone)]
pub struct BrandingFrames {
    pub black_key: Vec<u8>,
    pub logo_keys: HashMap<usize, Vec<u8>>,
    pub touchscreen_jpeg: Option<Vec<u8>>,
    pub neo_info_bar_jpeg: Option<Vec<u8>>,
}

/// Composites an RGBA image onto a pure `#000000` black canvas so transparent
/// pixels (even with non-zero RGB channels) always become crisp black.
fn composite_over_black(rgba: &RgbaImage) -> RgbImage {
    let (w, h) = rgba.dimensions();
    let mut out = ImageBuffer::new(w, h);
    for (x, y, px) in rgba.enumerate_pixels() {
        let a = px[3] as u16;
        let r = ((px[0] as u16 * a + 127) / 255) as u8;
        let g = ((px[1] as u16 * a + 127) / 255) as u8;
        let b = ((px[2] as u16 * a + 127) / 255) as u8;
        out.put_pixel(x, y, Rgb([r, g, b]));
    }
    out
}

fn load_logo_rgb() -> &'static RgbImage {
    DECODED_LOGO_RGB.get_or_init(|| {
        let img = image::load_from_memory(LOGO_2X2_PNG)
            .expect("Embedded resources/branding/logo-2x2.png must be a valid PNG")
            .to_rgba8();
        composite_over_black(&img)
    })
}

fn load_touchstrip_rgb() -> &'static RgbImage {
    DECODED_TOUCHSTRIP_RGB.get_or_init(|| {
        let img = image::load_from_memory(TOUCHSTRIP_PNG)
            .expect("Embedded resources/branding/touchstrip.png must be a valid PNG")
            .to_rgba8();
        composite_over_black(&img)
    })
}

/// Computes the top-left `(row, col)` of the centered 2x2 key block for any visual Stream Deck.
pub fn center_2x2_origin(kind: DeviceKind) -> Option<(usize, usize)> {
    let (rows, cols) = kind.grid_dimensions();
    if rows < 2 || cols < 2 {
        return None;
    }
    Some(((rows - 2) / 2, (cols - 2) / 2))
}

/// Renders and caches the native hardware-encoded Ballista branding frames for `kind`.
pub fn branding_frames_for(kind: DeviceKind) -> BrandingFrames {
    let cache = RENDER_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(existing) = cache.lock().unwrap().get(&kind).cloned() {
        return existing;
    }

    let built = build_branding_frames(kind);
    cache.lock().unwrap().insert(kind, built.clone());
    built
}

fn build_branding_frames(kind: DeviceKind) -> BrandingFrames {
    let format = kind.key_image_format();
    let (rot_ccw, flip_x, flip_y) = kind.key_image_transform();

    let (black_key, logo_keys) = if let Some((kw, kh)) = kind.key_image_size() {
        let black_img: RgbImage = ImageBuffer::from_pixel(kw, kh, Rgb([0, 0, 0]));
        let black_encoded = encode_hardware_image(&black_img, format, 90);

        let mut map = HashMap::new();
        if let Some((start_row, start_col)) = center_2x2_origin(kind) {
            let (_, cols) = kind.grid_dimensions();
            let logo = load_logo_rgb();
            // Render edge-to-edge across the 2x2 key matrix with no added outer margin
            let scaled = image::imageops::resize(logo, kw * 2, kh * 2, FilterType::Lanczos3);

            let quadrants = [
                (0u32, 0u32, start_row * cols + start_col),
                (kw, 0u32, start_row * cols + start_col + 1),
                (0u32, kh, (start_row + 1) * cols + start_col),
                (kw, kh, (start_row + 1) * cols + start_col + 1),
            ];

            for (qx, qy, key_idx) in quadrants {
                let tile = image::imageops::crop_imm(&scaled, qx, qy, kw, kh).to_image();
                let transformed = apply_hardware_transform(tile, rot_ccw, flip_x, flip_y);
                let encoded = encode_hardware_image(&transformed, format, 95);
                map.insert(key_idx, encoded);
            }
        }
        (black_encoded, map)
    } else {
        (Vec::new(), HashMap::new())
    };

    // Touch strip (Stream Deck + = 800x100 center-cropped from 1200x100; Plus XL = full 1200x100)
    let touchscreen_jpeg = kind.touchscreen_size().map(|(tw, th)| {
        let strip_src = load_touchstrip_rgb();
        // Normalize source (2000x166, ~12:1) to full 1200x100 scale so typography stays identical
        // physical size on both Stream Deck + (800x100) and Stream Deck + XL (1200x100).
        let full_w = (th * 12).max(tw);
        let scaled = image::imageops::resize(strip_src, full_w, th, FilterType::Lanczos3);
        let cropped = if full_w > tw {
            let x_off = (full_w - tw) / 2;
            image::imageops::crop_imm(&scaled, x_off, 0, tw, th).to_image()
        } else {
            scaled
        };
        let transformed =
            apply_hardware_transform(cropped, kind.touchscreen_rotation_ccw(), false, false);
        encode_hardware_image(&transformed, KeyImageFormat::Jpeg, 95)
    });

    // Stream Deck Neo 248x58 Info Bar (crop around centered "Ballista" wordmark and flip 180°)
    let neo_info_bar_jpeg = kind.info_screen_size().map(|(nw, nh)| {
        let strip_src = load_touchstrip_rgb();
        // Scale height to fit Neo info bar with comfortable margins, then center-crop width
        let scaled_w = ((strip_src.width() as f64 / strip_src.height().max(1) as f64) * nh as f64)
            .round()
            .max(nw as f64) as u32;
        let scaled = image::imageops::resize(strip_src, scaled_w, nh, FilterType::Lanczos3);
        let x_off = (scaled_w.saturating_sub(nw)) / 2;
        let cropped = image::imageops::crop_imm(&scaled, x_off, 0, nw, nh).to_image();
        let transformed = apply_hardware_transform(cropped, 0, true, true);
        encode_hardware_image(&transformed, KeyImageFormat::Jpeg, 95)
    });

    BrandingFrames {
        black_key,
        logo_keys,
        touchscreen_jpeg,
        neo_info_bar_jpeg,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_branding_assets_decode_and_composite_cleanly() {
        let logo = load_logo_rgb();
        assert_eq!(logo.dimensions(), (1002, 1002));
        // Transparent corners in source PNG must composite to pure black
        assert_eq!(*logo.get_pixel(0, 0), Rgb([0, 0, 0]));
        assert_eq!(*logo.get_pixel(1001, 1001), Rgb([0, 0, 0]));
        // Foreground pixels should use the muted silver-grey palette (~#8C8C90), not harsh pure white
        let max_logo_ch = logo.pixels().flat_map(|p| p.0).max().unwrap_or(0);
        assert!(
            (120..=170).contains(&max_logo_ch),
            "expected muted grey logo foreground, got max channel {max_logo_ch}"
        );

        let strip = load_touchstrip_rgb();
        assert_eq!(strip.dimensions(), (2000, 166));
        assert_eq!(*strip.get_pixel(0, 0), Rgb([0, 0, 0]));
        let max_strip_ch = strip.pixels().flat_map(|p| p.0).max().unwrap_or(0);
        assert!(
            (120..=170).contains(&max_strip_ch),
            "expected muted grey touchstrip foreground, got max channel {max_strip_ch}"
        );
    }

    #[test]
    fn center_2x2_key_indices_match_xl_and_plus() {
        // Stream Deck XL: 4x8 -> rows 1..=2, cols 3..=4 -> keys 11, 12, 19, 20
        let xl = branding_frames_for(DeviceKind::StreamDeckXL);
        let mut xl_keys: Vec<usize> = xl.logo_keys.keys().copied().collect();
        xl_keys.sort();
        assert_eq!(xl_keys, vec![11, 12, 19, 20]);
        assert!(!xl.black_key.is_empty());
        assert!(xl.touchscreen_jpeg.is_none());

        // Stream Deck +: 2x4 -> rows 0..=1, cols 1..=2 -> keys 1, 2, 5, 6 + 800x100 strip
        let plus = branding_frames_for(DeviceKind::StreamDeckPlus);
        let mut plus_keys: Vec<usize> = plus.logo_keys.keys().copied().collect();
        plus_keys.sort();
        assert_eq!(plus_keys, vec![1, 2, 5, 6]);
        assert!(plus.touchscreen_jpeg.as_ref().is_some_and(|b| !b.is_empty()));

        // Stream Deck + XL: 4x9 -> 1200x100 rotated strip
        let plus_xl = branding_frames_for(DeviceKind::StreamDeckPlusXL);
        assert_eq!(plus_xl.logo_keys.len(), 4);
        assert!(plus_xl.touchscreen_jpeg.as_ref().is_some_and(|b| !b.is_empty()));

        // Stream Deck Neo: 2x4 + 248x58 info bar
        let neo = branding_frames_for(DeviceKind::StreamDeckNeo);
        assert_eq!(neo.logo_keys.len(), 4);
        assert!(neo.neo_info_bar_jpeg.as_ref().is_some_and(|b| !b.is_empty()));
    }
}
