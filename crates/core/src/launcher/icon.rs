// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/src/launcher/icon.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Launcher icons: previews for the UI (data: URLs) and conversion
//           of PNG / ICO / JPEG / BMP into a square 256x256 PNG, which every
//           Ubuntu desktop renders sharply. SVG is installed as-is (scalable);
//           XPM is installed as-is (GNOME reads it, it cannot be previewed).
// =============================================================================

use std::io::Cursor;
use std::path::Path;

use image::{imageops::FilterType, DynamicImage, GenericImageView, ImageFormat, RgbaImage};

use crate::error::{CoreError, Result};

pub const ICON_SIZE: u32 = 256;
const MAX_SVG: u64 = 1024 * 1024;

/// Installed icon payload.
#[derive(Debug, Clone, PartialEq)]
pub enum IconData {
    Png(Vec<u8>),
    Svg(Vec<u8>),
    Xpm(Vec<u8>),
}

impl IconData {
    pub fn extension(&self) -> &'static str {
        match self {
            IconData::Png(_) => "png",
            IconData::Svg(_) => "svg",
            IconData::Xpm(_) => "xpm",
        }
    }
    pub fn bytes(&self) -> &[u8] {
        match self {
            IconData::Png(b) | IconData::Svg(b) | IconData::Xpm(b) => b,
        }
    }
}

fn ext(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

fn load_raster(path: &Path) -> Result<DynamicImage> {
    let bytes = std::fs::read(path)?;
    let fmt = match ext(path).as_str() {
        "ico" => ImageFormat::Ico,
        "jpg" | "jpeg" => ImageFormat::Jpeg,
        "bmp" => ImageFormat::Bmp,
        _ => ImageFormat::Png,
    };
    image::load_from_memory_with_format(&bytes, fmt)
        .or_else(|_| image::load_from_memory(&bytes))
        .map_err(|e| CoreError::Other(format!("cannot read icon {}: {e}", path.display())))
}

fn png_bytes(img: &DynamicImage) -> Result<Vec<u8>> {
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, ImageFormat::Png).map_err(|e| CoreError::Other(e.to_string()))?;
    Ok(out.into_inner())
}

/// Fit into a transparent square of `size` pixels, keeping the aspect ratio.
fn square(img: &DynamicImage, size: u32) -> DynamicImage {
    let (w, h) = img.dimensions();
    let scale = size as f32 / w.max(h).max(1) as f32;
    let (nw, nh) = (((w as f32 * scale).round() as u32).max(1), ((h as f32 * scale).round() as u32).max(1));
    let resized = img.resize_exact(nw, nh, FilterType::Lanczos3);
    let mut canvas = RgbaImage::new(size, size);
    image::imageops::overlay(&mut canvas, &resized.to_rgba8(), ((size - nw) / 2) as i64, ((size - nh) / 2) as i64);
    DynamicImage::ImageRgba8(canvas)
}

/// The icon to install for a source file.
pub fn prepare(path: &Path) -> Result<IconData> {
    match ext(path).as_str() {
        "svg" => {
            let m = std::fs::metadata(path)?;
            if m.len() > MAX_SVG {
                return Err(CoreError::Refused("SVG icon is larger than 1 MB".into()));
            }
            let b = std::fs::read(path)?;
            if !String::from_utf8_lossy(&b[..b.len().min(2048)]).contains("<svg") {
                return Err(CoreError::Refused("the file is not an SVG image".into()));
            }
            Ok(IconData::Svg(b))
        }
        "xpm" => {
            let b = std::fs::read(path)?;
            if b.len() as u64 > MAX_SVG {
                return Err(CoreError::Refused("XPM icon is too large".into()));
            }
            Ok(IconData::Xpm(b))
        }
        _ => {
            let img = load_raster(path)?;
            Ok(IconData::Png(png_bytes(&square(&img, ICON_SIZE))?))
        }
    }
}

/// (data URL, width, height) for the UI.
pub fn preview(path: &Path) -> Option<(Option<String>, u32, u32)> {
    match ext(path).as_str() {
        "svg" => {
            let b = std::fs::read(path).ok()?;
            if b.len() as u64 > MAX_SVG {
                return Some((None, 0, 0));
            }
            Some((Some(format!("data:image/svg+xml;base64,{}", crate::util::base64(&b))), 0, 0))
        }
        "xpm" => Some((None, 0, 0)),
        _ => {
            let img = load_raster(path).ok()?;
            let (w, h) = img.dimensions();
            let thumb = if w > 96 || h > 96 { square(&img, 96) } else { img };
            let png = png_bytes(&thumb).ok()?;
            Some((Some(format!("data:image/png;base64,{}", crate::util::base64(&png))), w, h))
        }
    }
}

/// Encode raw RGBA pixels as PNG (tests and generated icons).
pub fn encode_png_rgba(w: u32, h: u32, rgba: &[u8]) -> Result<Vec<u8>> {
    let img = RgbaImage::from_raw(w, h, rgba.to_vec()).ok_or_else(|| CoreError::Other("bad pixel buffer".into()))?;
    png_bytes(&DynamicImage::ImageRgba8(img))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_to_square_png() {
        let dir = std::env::temp_dir().join(format!("syspulse-icon-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("wide.png");
        std::fs::write(&src, encode_png_rgba(4, 2, &[200u8; 32]).unwrap()).unwrap();
        let IconData::Png(png) = prepare(&src).unwrap() else { panic!("png expected") };
        let img = image::load_from_memory(&png).unwrap();
        assert_eq!(img.dimensions(), (ICON_SIZE, ICON_SIZE));
        // transparent padding above the wide image
        assert_eq!(img.get_pixel(0, 0)[3], 0);
        assert!(preview(&src).unwrap().0.unwrap().starts_with("data:image/png;base64,"));

        let svg = dir.join("a.svg");
        std::fs::write(&svg, "<svg xmlns='http://www.w3.org/2000/svg'/>").unwrap();
        assert!(matches!(prepare(&svg).unwrap(), IconData::Svg(_)));
        let fake = dir.join("b.svg");
        std::fs::write(&fake, "hello").unwrap();
        assert!(prepare(&fake).is_err());
        std::fs::remove_dir_all(dir).ok();
    }
}
