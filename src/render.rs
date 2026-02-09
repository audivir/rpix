use anyhow::{Context, Result};
use image::{DynamicImage, GenericImage, RgbaImage};
use std::path::Path;
use std::process::{Command,Stdio};
#[cfg(feature = "pdf")]
use pdfium_render::prelude::{PdfRenderConfig, Pdfium};

#[cfg(any(feature = "html", feature = "office"))]
use directories::ProjectDirs;

#[cfg(feature = "html")]
use crate::{InputType, RpixContext};
#[cfg(feature = "html")]
use base64::{engine::general_purpose, Engine as _};
#[cfg(feature = "html")]
use std::path::PathBuf;

#[cfg(feature = "html")]
use headless_chrome::protocol::cdp::Page::CaptureScreenshotFormatOption;
#[cfg(feature = "html")]
use headless_chrome::{Browser, LaunchOptions};

#[cfg(feature = "office")]
use sha2::{Digest, Sha256};


#[cfg(test)]
mod tests_render;

struct Subdirs {
    cache_dir: PathBuf,
    data_dir: PathBuf,
}

#[cfg(not(target_os = "macos"))]
fn xdg_pref_project_dirs() -> Subdirs {
    // use original directories implementation
    let project_dirs = ProjectDirs::from("org", "example", "rpix").expect("Could not determine XDG directories");
    Subdirs {
        cache_dir: project_dirs.cache_dir().to_path_buf(),
        data_dir: project_dirs.data_dir().to_path_buf(),
    }
}

#[cfg(target_os = "macos")]
fn xdg_pref_project_dirs() -> Subdirs {
    // use macos home dir, but linux style paths
    let home_dir = directories::home_dir().expect("Could not determine home directory");

    let cache_dir = env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home_dir.join(".cache"));

    let data_dir = env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home_dir.join(".local/share"));

    Subdirs {
        cache_dir,
        data_dir,
    }
}


#[cfg(feature = "svg")]
pub fn render_svg(data: &[u8]) -> Result<DynamicImage> {
    // load system fonts
    let mut fontdb = usvg::fontdb::Database::new();
    fontdb.load_system_fonts();

    // configure options
    let opt = usvg::Options {
        fontdb: std::sync::Arc::new(fontdb),
        ..Default::default()
    };

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

#[cfg(feature = "pdf")]
pub fn render_pdf(
    data: &[u8],
    conf_w: Option<u32>,
    term_width: u32,
    page_indices: Option<Vec<u16>>,
) -> Result<DynamicImage> {
    let width = conf_w
        .unwrap_or(term_width)
        .try_into()
        .context("Failed to convert width to i32")?;

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
        .set_target_width(width)
        .render_form_data(true);
    let document = pdfium.load_pdf_from_byte_slice(data, None)?;
    let pages = document.pages();
    let n_pages = pages.len();
    let selected_indices = if let Some(page_indices) = page_indices {
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

#[cfg(feature = "html")]
fn is_url(s: &[u8]) -> bool {
    s.starts_with(b"http://") || s.starts_with(b"https://") || s.starts_with(b"file://")
}

#[cfg(feature = "html")]
fn is_url_str(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://") || s.starts_with("file://")
}

#[cfg(feature = "html")]
pub fn is_html(ctx: &RpixContext, extension: &str, s: &[u8]) -> bool {
    ctx.input_type == InputType::Html || extension == "html" || extension == "htm" || is_url(s)
}

#[cfg(feature = "html")]
pub fn render_html_chrome(data: &[u8]) -> Result<DynamicImage> {
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

    let user_data_dir = xdg_pref_project_dirs().data_dir.join("chromium");
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
    Ok(image::load_from_memory(&png_data)?)
}



#[cfg(target_os = "windows")]
mod win;
#[cfg(target_os = "windows")]
use win as sys;

#[cfg(feature = "office")]
pub fn render_office(
    data: &[u8],
    extension: &str,
    conf_w: Option<u32>,
    term_width: u32,
    pages: Option<Vec<u16>>,
    cache_dir: Option<&Path>,
) -> Result<DynamicImage> {
    let hash = Sha256::digest(data);
    let hash_str = hex::encode(hash);

    // convert to pdf with libreoffice (soffice command)
    let project_dirs = xdg_pref_project_dirs();
    let cache_dir = cache_dir.unwrap_or_else(|| &project_dirs.cache_dir);
    std::fs::create_dir_all(cache_dir)?;

    let cache_path = cache_dir.join(format!("{}.pdf", hash_str));
    if cache_path.exists() {
        // read cache data
        let cache_data = std::fs::read(&cache_path)?;
        return render_pdf(&cache_data, conf_w, term_width, pages);
    }

    // create temp file with name hash.extension
    let temp_dir = tempfile::tempdir()?;
    let source_temp = temp_dir.path().join(format!("{}.{}", hash_str, extension));
    std::fs::write(&source_temp, data)?;

    eprintln!("Converting office document to PDF...");
    // mute soffice output
    Command::new("soffice")
        .arg("--headless")
        .arg("--convert-to")
        .arg("pdf")
        .arg(source_temp.as_os_str())
        .arg("--outdir")
        .arg(cache_dir.as_os_str())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("Failed to convert office document to PDF")?;

    let pdf_path = cache_dir.join(format!("{}.pdf", hash_str));
    let pdf_data = std::fs::read(&pdf_path)?;
    render_pdf(&pdf_data, conf_w, term_width, pages)
}
