use image::codecs::gif::GifDecoder;
use image::AnimationDecoder;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct GifAnimation {
    pub frames: Vec<(Vec<u8>, u32)>, // (jpeg_bytes, delay_ms)
}

impl GifAnimation {
    pub fn load_key_gif(path: &str, dim: u32, rotate_180: bool) -> Option<Self> {
        if !path.to_lowercase().ends_with(".gif") || !Path::new(path).exists() {
            return None;
        }
        let file = File::open(path).ok()?;
        let decoder = GifDecoder::new(BufReader::new(file)).ok()?;
        let raw_frames = decoder.into_frames().collect_frames().ok()?;
        if raw_frames.len() <= 1 {
            return None;
        }

        let mut frames = Vec::new();
        for f in raw_frames {
            let (num, denom) = f.delay().numer_denom_ms();
            let delay_ms = if denom == 0 { num.max(30) } else { (num / denom).max(30) };
            let img = image::DynamicImage::ImageRgba8(f.into_buffer());
            let mut resized = img.resize_exact(dim, dim, image::imageops::FilterType::Triangle);
            if rotate_180 {
                resized = resized.rotate180();
            }
            let rgb = resized.to_rgb8();
            let mut jpeg = Vec::new();
            let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 90);
            let _ = enc.encode_image(&rgb);
            frames.push((jpeg, delay_ms));
        }

        Some(Self { frames })
    }

    pub fn load_touchscreen_gif(path: &str) -> Option<Self> {
        if !path.to_lowercase().ends_with(".gif") || !Path::new(path).exists() {
            return None;
        }
        let file = File::open(path).ok()?;
        let decoder = GifDecoder::new(BufReader::new(file)).ok()?;
        let raw_frames = decoder.into_frames().collect_frames().ok()?;
        if raw_frames.len() <= 1 {
            return None;
        }

        let mut frames = Vec::new();
        for f in raw_frames {
            let (num, denom) = f.delay().numer_denom_ms();
            let delay_ms = if denom == 0 { num.max(20) } else { (num / denom).max(20) };
            let img = image::DynamicImage::ImageRgba8(f.into_buffer());
            let resized = img.resize_exact(800, 100, image::imageops::FilterType::Lanczos3);
            let rgb = resized.to_rgb8();
            let mut jpeg = Vec::new();
            let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 95);
            let _ = enc.encode_image(&rgb);
            frames.push((jpeg, delay_ms));
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
