use anyhow::{Context, Result};
use image::{DynamicImage, Rgba};
use serde;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

mod config;
pub use config::*;

mod render;
pub use render::*;

mod send;
pub use send::*;

#[cfg(test)]
mod tests_lib;

static PLUGINS: OnceLock<std::collections::HashMap<String, Plugin>> = OnceLock::new();

#[derive(Debug)]
pub enum LoadResult {
    Image(DynamicImage),
    Data(Vec<u8>),
}

/// Defines how the image should be resized relative to the terminal or explicit dimensions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeMode {
    /// -n: Use the original image dimensions, even if they are larger than the terminal.
    Original,
    /// -r: Scale the terminal, both up or down, preserving aspect ratio.
    FitTerminal,
    /// -f: Force width to match terminal width, scaling height to preserve aspect ratio.
    FitWidth,
    /// -F: Force height to match terminal height, scaling width to preserve aspect ratio.
    FitHeight,
    /// -w / -H: Use one explicit dimension, scale the other to preserve aspect ratio.
    Manual {
        width: Option<u32>,
        height: Option<u32>,
    },
    /// Use the original size but clip the image to the terminal size.
    ClipTerminal,
}

/// Configuration for file caching (used for Office/PDF conversions).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheMode {
    /// Disable caching; use temporary directories that are cleaned up on exit.
    Disabled,
    /// Use the system default cache directory (e.g., ~/.cache/kv or %LOCALAPPDATA%\kv).
    Default,
    /// Use a specific custom directory.
    Custom(PathBuf),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InputType {
    Auto,
    Image,
    Text,
    Svg,
    Pdf,
    Html,
    Office,
}

#[derive(Debug, Clone)]
pub struct KvContext {
    pub input_type: InputType,
    pub resize_mode: ResizeMode,
    /// The detected terminal size (width, height).
    pub term_size: (u32, u32),
    pub page_indices: Option<Vec<u16>>,
    pub cache_mode: CacheMode,
    pub background_color: Option<Rgba<u8>>,
}

/// Detects terminal size with fallbacks.
pub fn get_term_size() -> (u32, u32) {
    let fallback = (800, 400);

    match crossterm::terminal::window_size() {
        Ok(size) => {
            let cols = size.columns as u32;
            let rows = size.rows as u32;

            let width = if size.width > 0 {
                size.width as u32
            } else if cols > 0 {
                cols * 10
            } else {
                fallback.0
            };

            let height = if size.height > 0 {
                let h = size.height as u32;
                // adjust for prompt line and padding if we have column info
                if cols > 0 {
                    h * (rows.saturating_sub(2)) / rows
                } else {
                    h
                }
            } else if rows > 0 {
                (rows.saturating_sub(2)) * 20
            } else {
                fallback.1
            };

            (width, height)
        }
        Err(_) => fallback,
    }
}

/// Parses a hex string (e.g., "#FFFFFF" or "FFFFFF") into an Rgba color.
pub fn parse_color(color: &str) -> Result<Rgba<u8>> {
    let hex = color.trim_start_matches('#');

    if hex.len() != 6 {
        anyhow::bail!("Invalid color format: must be 6 hex characters (e.g. #FFFFFF)");
    }

    let r = u8::from_str_radix(&hex[0..2], 16).context("Invalid Red component")?;
    let g = u8::from_str_radix(&hex[2..4], 16).context("Invalid Green component")?;
    let b = u8::from_str_radix(&hex[4..6], 16).context("Invalid Blue component")?;

    Ok(Rgba([r, g, b, 255]))
}

/// Calculates the final dimensions of the image based on the ResizeMode and Terminal Size.
pub fn calculate_dimensions(
    img_dims: (u32, u32),
    mode: ResizeMode,
    term_size: (u32, u32),
) -> (u32, u32) {
    let (w, h) = (img_dims.0 as f64, img_dims.1 as f64);
    let (tw, th) = (term_size.0 as f64, term_size.1 as f64);

    // calculate aspect ratio scaling
    let scale_to_width = |target_w: f64| (target_w, h * (target_w / w));
    let scale_to_height = |target_h: f64| (w * (target_h / h), target_h);

    let (final_w, final_h) = match mode {
        ResizeMode::Original => (w, h),

        ResizeMode::FitTerminal | ResizeMode::ClipTerminal => {
            // if clip terminal is enabled, scale only if the image is larger than the terminal
            if mode == ResizeMode::FitTerminal || (tw > 0.0 && w > tw) || (th > 0.0 && h > th) {
                let ratio = (tw / w).min(th / h);
                (w * ratio, h * ratio)
            } else {
                (w, h)
            }
        }

        ResizeMode::FitWidth => {
            if tw > 0.0 {
                scale_to_width(tw)
            } else {
                (w, h)
            }
        }

        ResizeMode::FitHeight => {
            if th > 0.0 {
                scale_to_height(th)
            } else {
                (w, h)
            }
        }

        ResizeMode::Manual { width, height } => match (width, height) {
            (Some(target_w), Some(target_h)) => (target_w as f64, target_h as f64),
            (Some(target_w), None) => scale_to_width(target_w as f64),
            (None, Some(target_h)) => scale_to_height(target_h as f64),
            (None, None) => (w, h), // should not happen
        },
    };

    (final_w.round() as u32, final_h.round() as u32)
}

/// Parse a 1-indexed pages string (e.g., "1-3,5") to 0-indexed vector.
pub fn parse_pages(pages: &str) -> Result<Option<Vec<u16>>> {
    if pages.trim().is_empty() {
        return Ok(None);
    }

    let mut result = Vec::new();

    for part in pages.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }

        if let Some((start_str, end_str)) = part.split_once('-') {
            let start: u16 = start_str
                .trim()
                .parse()
                .context("Invalid page range start")?;
            let end: u16 = end_str.trim().parse().context("Invalid page range end")?;

            if start < 1 || end <= start {
                anyhow::bail!("Page range must start >= 1 and end > start");
            }
            result.extend((start..=end).map(|i| i - 1));
        } else {
            let index: u16 = part.parse().context("Invalid page index")?;
            if index < 1 {
                anyhow::bail!("Page index must be >= 1");
            }
            result.push(index - 1);
        }
    }

    if result.is_empty() {
        Ok(None)
    } else {
        result.sort_unstable();
        result.dedup();
        Ok(Some(result))
    }
}

pub fn load_file(ctx: &KvContext, path: &Path) -> Result<LoadResult> {
    // handle extensions, might fail if non-UTF8
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    {
        // string conversion for URL check
        let path_lossy = path.to_string_lossy();
        if is_html(ctx, &extension, path_lossy.as_bytes()) {
            // use the bytes of the path string strictly for HTML rendering
            let img = render_html_chrome(ctx, path_lossy.as_bytes())?;
            return Ok(LoadResult::Image(img));
        }
    }

    let mut file =
        File::open(path).with_context(|| format!("Failed to open file: {}", path.display()))?;
    let mut data = Vec::new();
    file.read_to_end(&mut data)?;

    load_data(ctx, &data, &extension)
}

pub fn load_data(ctx: &KvContext, data: &[u8], extension: &str) -> Result<LoadResult> {
    if ctx.input_type == InputType::Text {
        return Ok(LoadResult::Data(data.to_vec()));
    }

    let plugins = PLUGINS.get_or_init(load_plugins);

    for plugin in plugins.values() {
        if has_extension_or_magic_bytes(
            data,
            extension,
            plugin.magic_bytes.as_ref().unwrap_or(&vec![]),
            &plugin.extensions,
        ) {
            return Ok(LoadResult::Image(render_plugin(ctx, data, plugin)?));
        }
    }

    if ctx.input_type == InputType::Image {
        let img = image::load_from_memory(data).context("Failed to load image")?;
        return Ok(LoadResult::Image(render_image(ctx, img)?));
    }

    if ctx.input_type == InputType::Svg
        || extension == "svg"
        || data.starts_with(b"<svg")
        || data.starts_with(b"<?xml")
    {
        return Ok(LoadResult::Image(render_svg(ctx, data)?));
    }

    if ctx.input_type == InputType::Pdf || extension == "pdf" || data.starts_with(b"%PDF") {
        return Ok(LoadResult::Image(render_pdf(ctx, data)?));
    }
    if ctx.input_type == InputType::Office
        || ["doc", "docx", "xls", "xlsx", "ppt", "pptx"].contains(&extension)
    {
        return Ok(LoadResult::Image(render_office(ctx, data, extension)?));
    }

    if is_html(ctx, extension, data)
        || data.starts_with(b"<html")
        || data.starts_with(b"<!DOCTYPE html")
    {
        return Ok(LoadResult::Image(render_html_chrome(ctx, data)?));
    }

    // fallback for InputType::Auto
    match image::load_from_memory(data) {
        Ok(img) => Ok(LoadResult::Image(render_image(ctx, img)?)),
        Err(err) => {
            // check if it's a valid UTF-8 string that points to a file path
            if let Ok(text) = std::str::from_utf8(data) {
                let path_str = text.trim();
                // Posix paths might contain \n, but this is so rare, we can ignore it for now
                // check to avoid treating random text as paths
                if !path_str.contains('\n') && !path_str.is_empty() {
                    let path = PathBuf::from(path_str);
                    if path.exists() && path.is_file() {
                        return load_file(ctx, &path);
                    }
                }
                // determine it is just text data
                return Ok(LoadResult::Data(data.to_vec()));
            }
            Err(anyhow::anyhow!("Failed to decode input: {}", err))
        }
    }
}
