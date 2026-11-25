use crate::models::{DeviceKind, KeyImageFormat};
use image::codecs::gif::GifDecoder;
use image::{AnimationDecoder, RgbImage};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

/// Applies hardware orientation transform in exact python-elgato-streamdeck PILHelper order:
/// 1. Counter-clockwise rotation (PIL `rotate(angle)` is CCW -> Rust `rotate270` for 90° CCW)
/// 2. Horizontal flip (`flip[0]`)
/// 3. Vertical flip (`flip[1]`)
pub fn apply_hardware_transform(
    mut img: RgbImage,
    rotation_ccw: u16,
    flip_x: bool,
    flip_y: bool,
) -> RgbImage {
    img = match rotation_ccw % 360 {
        90 => image::imageops::rotate270(&img),
        180 => image::imageops::rotate180(&img),
        270 => image::imageops::rotate90(&img),
        _ => img,
    };
    if flip_x {
        img = image::imageops::flip_horizontal(&img);
    }
    if flip_y {
        img = image::imageops::flip_vertical(&img);
    }
    img
}

/// Encodes an RGB image into the native byte format required by a Stream Deck model (JPEG or 24-bit BMP)
pub fn encode_hardware_image(img: &RgbImage, format: KeyImageFormat, jpeg_quality: u8) -> Vec<u8> {
    let mut out = Vec::new();
    match format {
        KeyImageFormat::Bmp => {
            let mut encoder = image::codecs::bmp::BmpEncoder::new(&mut out);
            let _ = encoder.encode(
                img.as_raw(),
                img.width(),
                img.height(),
                image::ExtendedColorType::Rgb8,
            );
        }
        KeyImageFormat::Jpeg => {
            let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, jpeg_quality);
            let _ = encoder.encode_image(img);
        }
    }
    out
}

#[derive(Debug, Clone)]
pub struct GifAnimation {
    pub frames: Vec<(Vec<u8>, u32)>, // (encoded_bytes, delay_ms)
}

impl GifAnimation {
    pub fn load_key_gif(path: &str, kind: DeviceKind) -> Option<Self> {
        let (w, h) = kind.key_image_size()?;
        if !path.to_lowercase().ends_with(".gif") || !Path::new(path).exists() {
            return None;
        }
        let file = File::open(path).ok()?;
        let decoder = GifDecoder::new(BufReader::new(file)).ok()?;
        let raw_frames = decoder.into_frames().collect_frames().ok()?;
        if raw_frames.len() <= 1 {
            return None;
        }

        let (rot_ccw, flip_x, flip_y) = kind.key_image_transform();
        let format = kind.key_image_format();
        let mut frames = Vec::new();
        for f in raw_frames {
            let (num, denom) = f.delay().numer_denom_ms();
            let delay_ms = if denom == 0 { num.max(30) } else { (num / denom).max(30) };
            let img = image::DynamicImage::ImageRgba8(f.into_buffer());
            let resized = img.resize_exact(w, h, image::imageops::FilterType::Triangle).to_rgb8();
            let transformed = apply_hardware_transform(resized, rot_ccw, flip_x, flip_y);
            let encoded = encode_hardware_image(&transformed, format, 90);
            frames.push((encoded, delay_ms));
        }

        Some(Self { frames })
    }

    pub fn load_touchscreen_gif(path: &str, kind: DeviceKind) -> Option<Self> {
        let (w, h) = kind.touchscreen_size()?;
        if !path.to_lowercase().ends_with(".gif") || !Path::new(path).exists() {
            return None;
        }
        let file = File::open(path).ok()?;
        let decoder = GifDecoder::new(BufReader::new(file)).ok()?;
        let raw_frames = decoder.into_frames().collect_frames().ok()?;
        if raw_frames.len() <= 1 {
            return None;
        }

        let rot_ccw = kind.touchscreen_rotation_ccw();
        let mut frames = Vec::new();
        for f in raw_frames {
            let (num, denom) = f.delay().numer_denom_ms();
            let delay_ms = if denom == 0 { num.max(20) } else { (num / denom).max(20) };
            let img = image::DynamicImage::ImageRgba8(f.into_buffer());
            let resized = img.resize_exact(w, h, image::imageops::FilterType::Lanczos3).to_rgb8();
            let transformed = apply_hardware_transform(resized, rot_ccw, false, false);
            let encoded = encode_hardware_image(&transformed, KeyImageFormat::Jpeg, 95);
            frames.push((encoded, delay_ms));
        }

        Some(Self { frames })
    }
}

#[derive(Debug, Clone)]
pub struct RawGifFrame {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub delay_ms: u32,
}

impl GifAnimation {
    pub fn load_key_gif_raw(path: &str, dim: u32, radius: f64) -> Option<Vec<RawGifFrame>> {
        if !path.to_lowercase().ends_with(".gif") || !Path::new(path).exists() {
            return None;
        }
        let file = File::open(path).ok()?;
        let decoder = GifDecoder::new(BufReader::new(file)).ok()?;
        let raw_frames = decoder.into_frames().collect_frames().ok()?;
        if raw_frames.is_empty() {
            return None;
        }

        let mut frames = Vec::new();
        let r2 = (radius * radius) as f32;
        let dim_f = dim as f32;

        for f in raw_frames {
            let (num, denom) = f.delay().numer_denom_ms();
            let delay_ms = if denom == 0 { num.max(30) } else { (num / denom).max(30) };
            let img = image::DynamicImage::ImageRgba8(f.into_buffer());
            let resized = img.resize_exact(dim, dim, image::imageops::FilterType::Nearest);
            let mut rgba = resized.to_rgba8();

            if radius > 0.0 {
                for y in 0..dim {
                    for x in 0..dim {
                        let xf = x as f32 + 0.5;
                        let yf = y as f32 + 0.5;
                        let in_tl = xf < radius as f32 && yf < radius as f32 && (xf - radius as f32).powi(2) + (yf - radius as f32).powi(2) > r2;
                        let in_tr = xf > dim_f - radius as f32 && yf < radius as f32 && (xf - (dim_f - radius as f32)).powi(2) + (yf - radius as f32).powi(2) > r2;
                        let in_bl = xf < radius as f32 && yf > dim_f - radius as f32 && (xf - radius as f32).powi(2) + (yf - (dim_f - radius as f32)).powi(2) > r2;
                        let in_br = xf > dim_f - radius as f32 && yf > dim_f - radius as f32 && (xf - (dim_f - radius as f32)).powi(2) + (yf - (dim_f - radius as f32)).powi(2) > r2;
                        if in_tl || in_tr || in_bl || in_br {
                            rgba.put_pixel(x, y, image::Rgba([0, 0, 0, 0]));
                        }
                    }
                }
            }

            frames.push(RawGifFrame {
                rgba: rgba.into_raw(),
                width: dim,
                height: dim,
                delay_ms,
            });
        }

        Some(frames)
    }

    pub fn load_key_gif_ui(path: &str, dim: u32, radius: f64) -> Option<Vec<(gtk::gdk::Texture, u32)>> {
        use gtk::prelude::Cast;
        let raw_frames = Self::load_key_gif_raw(path, dim, radius)?;
        let mut textures = Vec::new();
        for f in raw_frames {
            let bytes = glib::Bytes::from(&f.rgba[..]);
            let texture = gtk::gdk::MemoryTexture::new(
                f.width as i32,
                f.height as i32,
                gtk::gdk::MemoryFormat::R8g8b8a8,
                &bytes,
                (f.width * 4) as usize,
            );
            textures.push((texture.upcast(), f.delay_ms));
        }
        Some(textures)
    }
}
