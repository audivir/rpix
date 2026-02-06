use anyhow::{Context, Result};
use image::{GenericImage, DynamicImage, Rgba, RgbaImage};
use std::fs::File;
use std::io::{Read};
use std::path::PathBuf;
use pdfium_render::prelude::{Pdfium, PdfRenderConfig};

#[cfg(test)]
mod tests_lib;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InputType {
    Auto,
    Image,
    Svg,
    Pdf,
}

pub fn get_term_width_pixels() -> u32 {
   
    if let Ok(size) = crossterm::terminal::window_size() {
        // try raw pixels
        if size.width > 0 {
            return size.width as u32;
        }
        
        // fallback: if 0 pixels, estimate based on columns
        if size.columns > 0 {
            return size.columns as u32 * 10; 
        }
    }
    
    // ultimate fallback
    800 
}

pub fn parse_color(color: &str) -> Result<Rgba<u8>> {
    let color = color.trim_start_matches('#'); // Allow #FFFFFF
    if color.len() != 6 {
        return Err(anyhow::anyhow!("Invalid color format: {}", color));
    }
    let r = u8::from_str_radix(&color[0..2], 16)?;
    let g = u8::from_str_radix(&color[2..4], 16)?;
    let b = u8::from_str_radix(&color[4..6], 16)?;
    Ok(Rgba([r, g, b, 255]))
}

pub fn add_background(img: &DynamicImage, color: &Rgba<u8>) -> DynamicImage {
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let mut bg = RgbaImage::new(w, h);

    let bg_r = color[0] as u32;
    let bg_g = color[1] as u32;
    let bg_b = color[2] as u32;

    for (src, dst) in rgba.pixels().zip(bg.pixels_mut()) {
        let alpha = src[3] as u32;

        // if opaque, just copy it
        if alpha == 255 {
            *dst = *src;
            continue;
        }

        // if transparent, use background color
        if alpha == 0 {
            *dst = *color;
            continue;
        }

        // manual blending: src * alpha + bg * (1 - alpha)
        let inv_alpha = 255 - alpha;

        let r = (src[0] as u32 * alpha + bg_r * inv_alpha) / 255;
        let g = (src[1] as u32 * alpha + bg_g * inv_alpha) / 255;
        let b = (src[2] as u32 * alpha + bg_b * inv_alpha) / 255;

        *dst = Rgba([r as u8, g as u8, b as u8, 255]);
    }

    DynamicImage::ImageRgba8(bg)
}

pub fn calculate_dimensions(
    img_dims: (u32, u32),
    conf_w: Option<u32>,
    conf_h: Option<u32>,
    fullwidth: bool,
    noresize: bool,
    term_width: u32,
) -> (u32, u32) {
    let (orig_w, orig_h) = img_dims;
    let mut width = conf_w.unwrap_or(0);
    let mut height = conf_h.unwrap_or(0);

    if width > 0 && height == 0 {
        height = ((orig_h as f64 * (width as f64 / orig_w as f64)).round()) as u32;
    } else if height > 0 && width == 0 {
        width = ((orig_w as f64 * (height as f64 / orig_h as f64)).round()) as u32;
    } else if fullwidth {
        width = term_width;
        height = ((orig_h as f64 * (width as f64 / orig_w as f64)).round()) as u32;
    } else if orig_w > term_width && !noresize && term_width > 0 {
        width = term_width;
        height = ((orig_h as f64 * (width as f64 / orig_w as f64)).round()) as u32;
    } else {
        width = orig_w;
        height = orig_h;
    }
    (width, height)
}


pub fn render_svg(data: &[u8]) -> Result<DynamicImage> {
    // load system fonts
    let mut fontdb = usvg::fontdb::Database::new();
    fontdb.load_system_fonts();

    // configure options
    let mut opt = usvg::Options::default();
    opt.fontdb = std::sync::Arc::new(fontdb);

    // parse the SVG
    let tree = usvg::Tree::from_data(data, &opt).context("Failed to parse SVG")?;

    // pixel buffer
    let size = tree.size().to_int_size();
    let mut pixmap = tiny_skia::Pixmap::new(size.width(), size.height())
        .ok_or_else(|| anyhow::anyhow!("Failed to create pixmap"))?;

    resvg::render(
        &tree,
        tiny_skia::Transform::default(),
        &mut pixmap.as_mut()
    );

    // convert to DynamicImage
    let buffer = RgbaImage::from_raw(size.width(), size.height(), pixmap.data().to_vec())
        .ok_or_else(|| anyhow::anyhow!("Failed buffer conversion"))?;

    Ok(DynamicImage::ImageRgba8(buffer))
}

fn render_pdf(data: &[u8]) -> Result<DynamicImage> {
    let pdfium = Pdfium::new(
        Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path("./"))
            .or_else(|_| Pdfium::bind_to_system_library())? //.context("Failed to bind to PDFium")?
    );

    let document = pdfium.load_pdf_from_byte_slice(data, None)?;

    let mut images: Vec<DynamicImage> = Vec::new();

    for page in document.pages().iter() {
        let bitmap = page
            .render_with_config(
                &PdfRenderConfig::new()
                    .set_target_width(2000)
                    .render_form_data(true),
            )?;

        let image = bitmap.as_image();

        images.push(image);
    }

    if images.is_empty() {
        anyhow::bail!("No pages found in PDF");
    }

    let max_width = images.iter().map(|img| img.width()).max().unwrap();
    let total_height = images.iter().map(|img| img.height()).sum::<u32>();

    let mut combined = RgbaImage::new(max_width, total_height);

    let mut current_y = 0;
    for img in images {
        let rgba = img.to_rgba8();
        combined.copy_from(&rgba, 0, current_y)?;
        current_y += rgba.height();
    }

    Ok(DynamicImage::ImageRgba8(combined))
}

pub fn load_file(path: &PathBuf, input_type: InputType) -> Result<DynamicImage> {
    let mut file = File::open(path).context("Failed to open file")?;
    let mut data = Vec::new();
    file.read_to_end(&mut data)?;

    let extension = path.extension().map_or("", |e| e.to_str().unwrap_or(""));
    load_data(data, input_type, extension)
}

pub fn load_data(data: Vec<u8>, input_type: InputType, extension: &str) -> Result<DynamicImage> {
    if input_type == InputType::Image {
        return image::load_from_memory(&data).context("Failed to load image");
    }

    if input_type == InputType::Svg || extension == "svg" || data.starts_with(b"<svg") || data.starts_with(b"<?xml") {
        return render_svg(&data);
    }

    if input_type == InputType::Pdf || extension == "pdf" || data.starts_with(b"%PDF") {
        return render_pdf(&data);
    }

    // fallback for input_type == InputType::Auto
    match image::load_from_memory(&data) {
        Ok(img) => Ok(img),
        Err(err) => {
            if let Ok(text) = std::str::from_utf8(&data) {
                let path_str = text.trim();
                let path = PathBuf::from(path_str);
                if path.exists() && path.is_file() {
                    return load_file(&path, input_type);
                }
            }
            Err(anyhow::anyhow!("Failed to decode input: {}", err))
        }
    }
}
