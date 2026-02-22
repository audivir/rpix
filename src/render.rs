use anyhow::{Context, Result};
use image::imageops::FilterType;
use image::{DynamicImage, GenericImage, GenericImageView, Rgba, RgbaImage};
use std::io::Write;
use std::process::{Command, Stdio};

use crate::{calculate_dimensions, kv_project_dirs, CacheMode, Plugin, ResizeMode};

use pdfium_render::prelude::{PdfRenderConfig, Pdfium};

use crate::{InputType, KvContext};
use base64::{engine::general_purpose, Engine as _};
use std::path::PathBuf;

use headless_chrome::protocol::cdp::Page::CaptureScreenshotFormatOption;
use headless_chrome::{Browser, LaunchOptions};

use sha2::{Digest, Sha256};

#[cfg(test)]
mod tests_render;

pub fn add_background(img: &DynamicImage, color: &Rgba<u8>) -> DynamicImage {
    let mut bg = RgbaImage::from_pixel(img.width(), img.height(), *color);
    let rgba = img.to_rgba8();

    for (dst, src) in bg.pixels_mut().zip(rgba.pixels()) {
        let alpha = src[3] as u32;
        if alpha == 255 {
            *dst = *src;
        } else if alpha > 0 {
            // manual blending: src * alpha + bg * (1 - alpha)
            let inv_alpha = 255 - alpha;
            let bg_r = dst[0] as u32;
            let bg_g = dst[1] as u32;
            let bg_b = dst[2] as u32;

            let r = (src[0] as u32 * alpha + bg_r * inv_alpha) / 255;
            let g = (src[1] as u32 * alpha + bg_g * inv_alpha) / 255;
            let b = (src[2] as u32 * alpha + bg_b * inv_alpha) / 255;

            *dst = Rgba([r as u8, g as u8, b as u8, 255]);
        }
    }

    DynamicImage::ImageRgba8(bg)
}

pub fn render_image(ctx: &KvContext, img: DynamicImage) -> Result<DynamicImage> {
    let (w, h) = calculate_dimensions(img.dimensions(), ctx.resize_mode, ctx.term_size);
    let mut final_img = img;

    if w != 0 && h != 0 && (w != final_img.width() || h != final_img.height()) {
        final_img = final_img.resize_exact(w, h, FilterType::Triangle);
    }

    if let Some(color) = ctx.background_color {
        final_img = add_background(&final_img, &color);
    }
    Ok(final_img)
}

pub fn render_svg(ctx: &KvContext, data: &[u8]) -> Result<DynamicImage> {
    let mut fontdb = usvg::fontdb::Database::new();
    fontdb.load_system_fonts();

    let opt = usvg::Options {
        fontdb: std::sync::Arc::new(fontdb),
        ..Default::default()
    };

    let tree = usvg::Tree::from_data(data, &opt).context("Failed to parse SVG")?;
    let size = tree.size().to_int_size();

    let (new_w, new_h) = calculate_dimensions(
        (size.width(), size.height()),
        ctx.resize_mode,
        ctx.term_size,
    );

    let mut pixmap = tiny_skia::Pixmap::new(new_w, new_h)
        .ok_or_else(|| anyhow::anyhow!("Failed to create pixmap"))?;

    if let Some(color) = ctx.background_color {
        pixmap.fill(tiny_skia::Color::from_rgba8(
            color[0], color[1], color[2], color[3],
        ));
    }

    let scale_x = new_w as f32 / size.width() as f32;
    let scale_y = new_h as f32 / size.height() as f32;
    let transform = tiny_skia::Transform::from_scale(scale_x, scale_y);

    resvg::render(&tree, transform, &mut pixmap.as_mut());

    let buffer = RgbaImage::from_raw(new_w, new_h, pixmap.data().to_vec())
        .ok_or_else(|| anyhow::anyhow!("Failed buffer conversion"))?;

    Ok(DynamicImage::ImageRgba8(buffer))
}

pub fn render_pdf(ctx: &KvContext, data: &[u8]) -> Result<DynamicImage> {
    let width = match ctx.resize_mode {
        ResizeMode::Manual { width: Some(w), .. } => w,
        ResizeMode::FitWidth | ResizeMode::FitTerminal => ctx.term_size.0,
        _ => {
            if ctx.term_size.0 > 0 {
                ctx.term_size.0
            } else {
                800
            }
        }
    };

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
            .or_else(|_| Pdfium::bind_to_system_library())?,
    );

    let config = PdfRenderConfig::new()
        .set_target_width(width.try_into().unwrap_or(800))
        .render_form_data(true);

    let document = pdfium.load_pdf_from_byte_slice(data, None)?;
    let pages = document.pages();
    let n_pages = pages.len();

    let selected_indices = if let Some(page_indices) = &ctx.page_indices {
        if page_indices.iter().any(|&i| i >= n_pages) {
            anyhow::bail!("Page index out of range (must be <= {})", n_pages);
        }
        page_indices.clone()
    } else {
        (0..n_pages).collect()
    };

    let mut images: Vec<RgbaImage> = Vec::with_capacity(selected_indices.len());
    for page_index in selected_indices {
        let page = pages
            .get(page_index)
            .context(format!("Failed to get page {}", page_index))?;
        let bitmap = page.render_with_config(&config)?;
        images.push(bitmap.as_image().to_rgba8());
    }

    if images.is_empty() {
        anyhow::bail!("No pages found in PDF");
    }

    let max_width = images.iter().map(|img| img.width()).max().unwrap_or(0);
    let total_height = images.iter().map(|img| img.height()).sum::<u32>();

    let mut combined = RgbaImage::new(max_width, total_height);
    let mut current_y = 0;
    for img in images {
        combined.copy_from(&img, 0, current_y)?;
        current_y += img.height();
    }

    render_image(ctx, DynamicImage::ImageRgba8(combined))
}

fn is_url(s: &[u8]) -> bool {
    s.starts_with(b"http://") || s.starts_with(b"https://") || s.starts_with(b"file://")
}

fn is_url_str(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://") || s.starts_with("file://")
}

pub fn is_html(ctx: &KvContext, extension: &str, s: &[u8]) -> bool {
    ctx.input_type == InputType::Html || extension == "html" || extension == "htm" || is_url(s)
}

pub fn render_html_chrome(ctx: &KvContext, data: &[u8]) -> Result<DynamicImage> {
    let data_str = std::str::from_utf8(data)?;
    let url: String = if is_url_str(data_str) {
        data_str.to_owned()
    } else {
        let path = PathBuf::from(data_str);
        if path.exists() {
            let absolute_path = path.canonicalize()?;
            format!("file://{}", absolute_path.display())
        } else {
            format!(
                "data:text/html;base64,{}",
                general_purpose::STANDARD.encode(data)
            )
        }
    };

    let user_data_dir = kv_project_dirs().data_dir.join("chromium");
    std::fs::create_dir_all(&user_data_dir)?;
    let browser = Browser::new(LaunchOptions {
        headless: true,
        path: None,
        user_data_dir: Some(user_data_dir),
        ..Default::default()
    })?;
    let tab = browser.new_tab()?;
    tab.navigate_to(&url)?;
    tab.wait_for_element("body")?;
    let png_data = tab.capture_screenshot(CaptureScreenshotFormatOption::Png, None, None, true)?;
    let img = image::load_from_memory(&png_data)?;
    render_image(ctx, img)
}

#[cfg(target_os = "windows")]
mod win;
#[cfg(target_os = "windows")]
use win as sys;

pub fn render_office(ctx: &KvContext, data: &[u8], extension: &str) -> Result<DynamicImage> {
    let hash = Sha256::digest(data);
    let hash_str = hex::encode(hash);

    // convert to pdf with libreoffice (soffice command)
    let project_dirs = kv_project_dirs();
    let temp_dir_guard = tempfile::tempdir()?; // Keep guard alive

    let (target_dir, is_persistent) = match &ctx.cache_mode {
        CacheMode::Disabled => (temp_dir_guard.path().to_path_buf(), false),
        CacheMode::Default => (project_dirs.cache_dir.clone(), true),
        CacheMode::Custom(path) => (path.clone(), true),
    };

    if is_persistent {
        std::fs::create_dir_all(&target_dir).context("Failed to create cache directory")?;

        // check if cached PDF already exists
        let cache_path = target_dir.join(format!("{}.pdf", hash_str));
        if cache_path.exists() {
            let cache_data = std::fs::read(&cache_path)?;
            return render_pdf(ctx, &cache_data);
        }
    }

    // create temp file with name hash.extension
    let source_temp = target_dir.join(format!("{}.{}", hash_str, extension));
    std::fs::write(&source_temp, data)?;

    eprintln!("Converting office document to PDF...");
    // mute soffice output
    Command::new("soffice")
        .arg("--headless")
        .arg("--convert-to")
        .arg("pdf")
        .arg(source_temp.as_os_str())
        .arg("--outdir")
        .arg(target_dir.as_os_str())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("Failed to convert office document to PDF")?;

    let pdf_path = target_dir.join(format!("{}.pdf", hash_str));
    let pdf_data = std::fs::read(&pdf_path)?;
    render_pdf(ctx, &pdf_data)
}

pub fn render_plugin(ctx: &KvContext, data: &[u8], plugin: &Plugin) -> Result<DynamicImage> {
    let temp_dir_guard = tempfile::tempdir()?;
    let mut command_parts =
        shell_words::split(&plugin.path).context("Invalid command string in plugin config")?;

    if command_parts.is_empty() {
        anyhow::bail!("Plugin command is empty");
    }

    let program = command_parts.remove(0);
    let mut cmd = Command::new(program);

    if let (Some(i), Some(o)) = (&plugin.placeholder, &plugin.output_placeholder) {
        if i == o {
            anyhow::bail!("Input placeholder equals output placeholder");
        }
    }

    let mut input_path_opts = None;
    if let Some(placeholder) = &plugin.placeholder {
        let path = temp_dir_guard.path().join("input_tmp");
        std::fs::write(&path, data)?;
        input_path_opts = Some((placeholder.clone(), path.to_string_lossy().to_string()));
    }

    let mut output_path_opts = None;
    if let Some(output_placeholder) = &plugin.output_placeholder {
        let path = temp_dir_guard.path().join("output_tmp");
        output_path_opts = Some((
            output_placeholder.clone(),
            path.to_string_lossy().to_string(),
        ));
    }

    let mut replaced_input = false;
    let mut replaced_output = false;
    let mut replacements = Vec::new();
    if let Some((p, path)) = &input_path_opts {
        replacements.push((p, path, &mut replaced_input));
    }
    if let Some((p, path)) = &output_path_opts {
        replacements.push((p, path, &mut replaced_output));
    }

    // This ensures "{{}}" is checked before "{}" automatically
    replacements.sort_by(|(p1, _, _), (p2, _, _)| p2.len().cmp(&p1.len()));

    for arg in command_parts {
        let mut final_arg = arg;
        for (placeholder, path, flag) in replacements.iter_mut() {
            if final_arg.contains(*placeholder) {
                final_arg = final_arg.replace(*placeholder, path);
                **flag = true;
            }
        }
        cmd.arg(final_arg);
    }

    if input_path_opts.is_some() && !replaced_input {
        anyhow::bail!("Input placeholder not found in arguments");
    }
    if output_path_opts.is_some() && !replaced_output {
        anyhow::bail!("Output placeholder not found in arguments");
    }

    // pipe outputs correctly
    if input_path_opts.is_none() {
        cmd.stdin(Stdio::piped());
    }
    if output_path_opts.is_none() {
        cmd.stdout(Stdio::piped());
    }
    cmd.stderr(Stdio::inherit());

    let mut child = cmd.spawn().context("Failed to spawn plugin command")?;

    if input_path_opts.is_none() {
        let mut stdin = child.stdin.take().context("Failed to open stdin")?;
        stdin
            .write_all(data)
            .context("Failed to write to plugin stdin")?;
        drop(stdin); // Close stdin to assure no more data
    }

    let output = child
        .wait_with_output()
        .context("Plugin execution failed")?;

    if !output.status.success() {
        anyhow::bail!("Plugin exited with error code: {:?}", output.status.code());
    }

    let output_data = if let Some((_, path)) = output_path_opts {
        std::fs::read(path).context("Failed to read plugin output file")?
    } else {
        output.stdout
    };

    // if output is empty raise error
    if output_data.is_empty() {
        anyhow::bail!("Plugin returned no output");
    }

    match plugin.output {
        InputType::Svg => render_svg(ctx, &output_data),
        InputType::Pdf => render_pdf(ctx, &output_data),
        InputType::Html => render_html_chrome(ctx, &output_data),
        _ => {
            let img = image::load_from_memory(&output_data)
                .context("Failed to decode plugin output as image")?;
            render_image(ctx, img)
        }
    }
}
