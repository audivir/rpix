use anyhow::{Context, Result};
use image::{DynamicImage, GenericImage, Rgba, RgbaImage};
use pdfium_render::prelude::{PdfRenderConfig, Pdfium};
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;

#[cfg(test)]
mod tests_lib;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InputType {
    Auto,
    Image,
    Svg,
    Pdf,
}

pub fn get_term_size() -> (u32, u32) {
    let mut width = 800; // ultimate fallback
    let mut height = 400; // ultimate fallback
    if let Ok(size) = crossterm::terminal::window_size() {
        // try raw pixels
        // fallback: if 0 pixels, estimate based on columns
        let cols = size.columns as u32;
        let rows = size.rows as u32;

        if size.width > 0 {
            width = size.width as u32;
        } else if cols > 0 {
            width = cols * 10;
        }
        // if possible adjust for the new prompt line and the empty line after the image
        if size.height > 0 {
            height = size.height as u32;
            if cols > 0 {
                height = height * (rows - 2) / rows;
            }
        } else if rows > 0 {
            height = (rows - 2) * 20;
        }
    }
    (width, height)
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
    fullheight: bool,
    resize: bool,
    noresize: bool,
    term_width: u32,
    term_height: u32,
) -> (u32, u32) {
    let (orig_w, orig_h) = (img_dims.0 as f64, img_dims.1 as f64);
    let mut width = conf_w.unwrap_or(0) as f64;
    let mut height = conf_h.unwrap_or(0) as f64;

    let mut use_resize = resize;
    let mut use_fullwidth = fullwidth;
    let mut use_fullheight = fullheight;

    // if neither noresize nor fullwidth nor fullheight is set,
    // then resize if the image is larger than the terminal
    if !noresize
        && !use_fullwidth
        && !use_fullheight
        && ((orig_w > term_width.into() && term_width > 0)
            || (orig_h > term_height.into() && term_height > 0))
    {
        use_resize = true;
    }

    if use_resize {
        let aspect_w = orig_w / orig_h;
        let aspect_h = orig_h / orig_w;
        if aspect_w > aspect_h {
            use_fullwidth = true;
        } else {
            use_fullheight = true;
        }
    }

    // if width or height is set, use it
    if width > 0.0 && height == 0.0 {
        height = orig_h * (width / orig_w);
    } else if height > 0.0 && width == 0.0 {
        width = orig_w * (height / orig_h);
    // use full terminal width, scale height by aspect ratio
    } else if use_fullwidth {
        width = term_width.into();
        height = orig_h * (width / orig_w);
    // use full terminal height, scale width by aspect ratio
    } else if use_fullheight {
        height = term_height.into();
        width = orig_w * (height / orig_h);
    // use original size
    } else {
        width = orig_w;
        height = orig_h;
    }
    (width.round() as u32, height.round() as u32)
}

// Parse a 1-indexed pages string to 0-indexed vector
pub fn parse_pages(pages: &str) -> Result<Vec<u16>> {
    let mut result = Vec::new();
    for part in pages.split(',') {
        let part = part.trim();
        if part.contains('-') {
            let mut parts = part.split('-');
            let start = parts
                .next()
                .context("Invalid page range")?
                .trim()
                .parse::<u16>()?;
            let end = parts
                .next()
                .context("Invalid page range")?
                .trim()
                .parse::<u16>()?;
            if start < 1 || end <= start {
                return Err(anyhow::anyhow!(
                    "Page range must start >= 1 and end > start"
                ));
            }
            for i in start..=end {
                result.push(i - 1);
            }
        } else {
            let index = part.parse::<u16>().context("Invalid page index")?;
            if index < 1 {
                return Err(anyhow::anyhow!("Page index must be >= 1"));
            }
            result.push(index - 1);
        }
    }
    // sort and deduplicate
    result.sort();
    result.dedup();
    Ok(result)
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

    resvg::render(&tree, tiny_skia::Transform::default(), &mut pixmap.as_mut());

    // convert to DynamicImage
    let buffer = RgbaImage::from_raw(size.width(), size.height(), pixmap.data().to_vec())
        .ok_or_else(|| anyhow::anyhow!("Failed buffer conversion"))?;

    Ok(DynamicImage::ImageRgba8(buffer))
}

fn render_pdf(
    data: &[u8],
    conf_w: Option<u32>,
    term_width: u32,
    page_indices: Option<Vec<u16>>,
) -> Result<DynamicImage> {
    let width = conf_w
        .unwrap_or(term_width)
        .try_into()
        .context("Failed to convert width to i32")?;

    // libraries to look for ./, ./pdfium/, /opt/homebrew/lib, /usr/local/lib, /usr/local/pdfium/lib
    let pdfium = Pdfium::new(
        Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path("./"))
            .or_else(|_| {
                Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path("./pdfium/"))
            })
            .or_else(|_| {
                Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path(
                    "/opt/homebrew/lib",
                ))
            })
            .or_else(|_| {
                Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path(
                    "/usr/local/lib",
                ))
            })
            .or_else(|_| {
                Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path(
                    "/usr/local/pdfium/lib",
                ))
            })
            .or_else(|_| Pdfium::bind_to_system_library())?,
    );

    let config = PdfRenderConfig::new()
        .set_target_width(width)
        .render_form_data(true);

    let document = pdfium.load_pdf_from_byte_slice(data, None)?;
    let pages = document.pages();
    let n_pages = pages.len() as u16;
    let selected_indices = if let Some(page_indices) = page_indices {
        // if any >= pages.len(), raise error
        if page_indices.iter().any(|&i| i >= n_pages) {
            anyhow::bail!("Page index out of range (must be <= {})", n_pages);
        }
        page_indices
    } else {
        (0..n_pages).collect()
    };

    let mut images: Vec<RgbaImage> = Vec::new();

    for page_index in selected_indices.iter() {
        let page = pages
            .get(*page_index)
            .context(format!("Failed to get page {}", page_index))?;
        let bitmap = page.render_with_config(&config)?;
        let image = bitmap.as_image().to_rgba8();
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
        combined.copy_from(&img, 0, current_y)?;
        current_y += img.height();
    }

    Ok(DynamicImage::ImageRgba8(combined))
}

pub fn load_file(
    path: &PathBuf,
    input_type: InputType,
    conf_w: Option<u32>,
    term_width: u32,
    page_indices: Option<Vec<u16>>,
) -> Result<DynamicImage> {
    let mut file = File::open(path).context("Failed to open file")?;
    let mut data = Vec::new();
    file.read_to_end(&mut data)?;

    let extension = path.extension().map_or("", |e| e.to_str().unwrap_or(""));
    load_data(
        data,
        input_type,
        extension,
        conf_w,
        term_width,
        page_indices,
    )
}

pub fn load_data(
    data: Vec<u8>,
    input_type: InputType,
    extension: &str,
    conf_w: Option<u32>,
    term_width: u32,
    page_indices: Option<Vec<u16>>,
) -> Result<DynamicImage> {
    if input_type == InputType::Image {
        return image::load_from_memory(&data).context("Failed to load image");
    }

    if input_type == InputType::Svg
        || extension == "svg"
        || data.starts_with(b"<svg")
        || data.starts_with(b"<?xml")
    {
        return render_svg(&data);
    }

    if input_type == InputType::Pdf || extension == "pdf" || data.starts_with(b"%PDF") {
        return render_pdf(&data, conf_w, term_width, page_indices);
    }

    // fallback for input_type == InputType::Auto
    match image::load_from_memory(&data) {
        Ok(img) => Ok(img),
        Err(err) => {
            if let Ok(text) = std::str::from_utf8(&data) {
                let path_str = text.trim();
                let path = PathBuf::from(path_str);
                if path.exists() && path.is_file() {
                    return load_file(&path, input_type, conf_w, term_width, page_indices);
                }
            }
            Err(anyhow::anyhow!("Failed to decode input: {}", err))
        }
    }
}
