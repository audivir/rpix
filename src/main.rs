use anyhow::Result;
use base64::{engine::general_purpose, Engine as _};
use clap::{Parser, ValueEnum};
use flate2::write::ZlibEncoder;
use flate2::Compression;
use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, ImageEncoder};
use rpix::*;
use std::io::{self, Read, Write};
use std::path::PathBuf;

const CHUNK_SIZE: usize = 4096;

#[cfg(test)]
mod tests_main;

#[derive(Debug, Clone, ValueEnum, PartialEq)]
enum Mode {
    Png,
    Zlib,
    Raw,
}

#[derive(Debug, Clone, ValueEnum, PartialEq)]
enum InputTypeOption {
    Auto,
    Image,
    Svg,
    Pdf,
}

impl From<InputTypeOption> for InputType {
    fn from(arg: InputTypeOption) -> Self {
        match arg {
            InputTypeOption::Auto => InputType::Auto,
            InputTypeOption::Image => InputType::Image,
            InputTypeOption::Svg => InputType::Svg,
            InputTypeOption::Pdf => InputType::Pdf,
        }
    }
}

/// A image viewer for the Kitty Terminal Graphics Protocol.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Config {
    /// Input files
    #[arg(name = "FILES")]
    files: Vec<PathBuf>,

    /// Specify image width
    #[arg(
        short = 'w',
        long,
        conflicts_with = "height",
        conflicts_with = "fullwidth",
        conflicts_with = "fullheight",
        conflicts_with = "resize",
        conflicts_with = "noresize"
    )]
    width: Option<u32>,

    /// Specify image height
    #[arg(
        short = 'H', // else conflicts with --help
        long,
        conflicts_with = "width",
        conflicts_with = "fullwidth",
        conflicts_with = "fullheight",
        conflicts_with = "resize",
        conflicts_with = "noresize"
    )]
    height: Option<u32>,

    /// Resize image to fill terminal width
    #[arg(
        short = 'f',
        long,
        conflicts_with = "width",
        conflicts_with = "height",
        conflicts_with = "fullheight",
        conflicts_with = "resize",
        conflicts_with = "noresize"
    )]
    fullwidth: bool,

    /// Resize image to fill terminal height
    #[arg(
        short = 'F',
        long,
        conflicts_with = "width",
        conflicts_with = "height",
        conflicts_with = "fullwidth",
        conflicts_with = "resize",
        conflicts_with = "noresize"
    )]
    fullheight: bool,

    /// Resize image to fill terminal
    #[arg(
        short = 'r',
        long,
        conflicts_with = "width",
        conflicts_with = "height",
        conflicts_with = "fullwidth",
        conflicts_with = "fullheight",
        conflicts_with = "noresize"
    )]
    resize: bool,

    /// Disable automatic resizing
    #[arg(
        short = 'n',
        long,
        conflicts_with = "width",
        conflicts_with = "height",
        conflicts_with = "fullwidth",
        conflicts_with = "fullheight",
        conflicts_with = "resize"
    )]
    noresize: bool,

    /// Add background if image is transparent
    #[arg(short = 'b', long)]
    background: bool,

    /// Background color as hex string
    #[arg(short = 'C', long, default_value = "FFFFFF", requires = "background")]
    color: String,

    /// Transmission mode
    #[arg(short = 'm', long, value_enum, default_value_t = Mode::Png)]
    mode: Mode,

    /// Input type
    #[arg(short = 'i', long, value_enum, default_value_t = InputTypeOption::Auto, conflicts_with = "pages")]
    input: InputTypeOption,

    /// Select which PDF pages to render, forces input type to pdf (e.g. "1-3,34")
    #[arg(short = 'P', long, conflicts_with = "input")]
    pages: Option<String>,

    /// Print file name
    #[arg(short = 'p', long)]
    printname: bool,

    /// Force tty (ignore stdin check)
    #[arg(short = 't', long)]
    tty: bool,

    /// Clear terminal (does not print image)
    #[arg(short = 'c', long)]
    clear: bool,
}

fn render_image(
    mut writer: impl Write,
    img: DynamicImage,
    conf: &Config,
    term_size: (u32, u32),
) -> Result<()> {
    let (w, h) = calculate_dimensions(
        img.dimensions(),
        conf.width,
        conf.height,
        conf.fullwidth,
        conf.fullheight,
        conf.resize,
        conf.noresize,
        term_size.0,
        term_size.1,
    );
    let mut final_img = img;

    if w != 0 && h != 0 && (w != final_img.width() || h != final_img.height()) {
        final_img = final_img.resize_exact(w, h, FilterType::Triangle);
    }

    if conf.background {
        match parse_color(&conf.color) {
            Ok(color) => final_img = add_background(&final_img, &color),
            Err(e) => return Err(e),
        }
    }

    let payload: Vec<u8>;
    let header: String;

    if conf.mode == Mode::Png {
        // encode as png
        let mut buffer = Vec::new();
        let encoder = image::codecs::png::PngEncoder::new(&mut buffer);
        let (width, height) = final_img.dimensions();
        let color_type = final_img.color();

        encoder.write_image(final_img.as_bytes(), width, height, color_type.into())?;

        payload = buffer;

        // f=100 (PNG), no width/height data
        header = "a=T,f=100,".to_string();
    } else {
        let (width, height) = final_img.dimensions();
        let raw_bytes = final_img.to_rgba8().into_raw();

        if conf.mode == Mode::Raw {
            payload = raw_bytes;
            // f=32 (RGBA)
            header = format!("a=T,f=32,s={},v={},", width, height);
        } else {
            // compress with zlib
            let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
            encoder.write_all(&raw_bytes)?;
            payload = encoder.finish()?;

            // f=32 (RGBA), o=z (compressed)
            header = format!("a=T,f=32,s={},v={},o=z,", width, height);
        }
    }

    // base64 encode payload
    let encoded = general_purpose::STANDARD.encode(&payload);

    let total_len = encoded.len();
    let mut pos = 0;
    let mut is_first = true;

    while pos < total_len {
        let end = std::cmp::min(pos + CHUNK_SIZE, total_len);
        let chunk = &encoded[pos..end];
        let more = if end < total_len { 1 } else { 0 };

        write!(writer, "\x1b_G")?;
        if is_first {
            write!(writer, "{}", header)?;
        }
        write!(writer, "m={};", more)?;
        writer.write_all(chunk.as_bytes())?;
        write!(writer, "\x1b\\")?;

        pos = end;
        is_first = false;
    }
    writeln!(writer)?;
    Ok(())
}

fn run(
    mut writer: impl Write,
    mut err_writer: impl Write,
    mut reader: impl Read,
    conf: Config,
    term_size: (u32, u32),
    is_input_available: bool,
) -> Result<i32> {
    if conf.clear {
        write!(writer, "\x1b_Ga=d\x1b\\")?;
        return Ok(0);
    }

    // If -t is passed, we ignore stdin even if input is available
    let use_stdin = is_input_available && !conf.tty;

    let (page_indices, input_type) = if conf.pages.is_some() {
        if !use_stdin && conf.files.len() > 1 {
            writeln!(
                err_writer,
                "Error: Cannot specify multiple files with --pages"
            )?;
            return Ok(1);
        }
        let page_indices = if let Ok(pages) = parse_pages(&conf.pages.as_ref().unwrap()) {
            pages
        } else {
            writeln!(err_writer, "Error: Invalid page range")?;
            return Ok(1);
        };
        let input_type = InputType::Pdf;
        (page_indices, input_type)
    } else {
        (None, conf.input.to_owned().into())
    };

    if use_stdin {
        let mut data = Vec::new();
        reader.read_to_end(&mut data)?;
        let img = load_data(data, input_type, "", conf.width, term_size.0, page_indices)?;
        if conf.printname {
            writeln!(err_writer, "stdin")?;
        }
        render_image(writer, img, &conf, term_size)?;
    } else if !conf.files.is_empty() {
        let mut exit_code = 0;
        for path in &conf.files {
            match load_file(
                path,
                input_type,
                conf.width,
                term_size.0,
                page_indices.clone(),
            ) {
                Ok(img) => {
                    if conf.printname {
                        writeln!(err_writer, "{}", path.display())?;
                    }
                    if let Err(e) = render_image(&mut writer, img, &conf, term_size) {
                        writeln!(err_writer, "Error rendering {}: {}", path.display(), e)?;
                        exit_code = 1;
                    }
                }
                Err(e) => {
                    writeln!(err_writer, "Error loading {}: {}", path.display(), e)?;
                    exit_code = 1;
                }
            }
        }
        return Ok(exit_code);
    } else {
        writeln!(
            err_writer,
            "Error: No input files provided and no data piped to stdin."
        )?;
        return Ok(1);
    }

    Ok(0)
}

fn main() -> Result<()> {
    let conf = Config::parse();
    let term_size = get_term_size();

    // Detect TTY status
    let is_input_available = atty::isnt(atty::Stream::Stdin);

    let code = run(
        io::stdout(),
        io::stderr(),
        io::stdin(),
        conf,
        term_size,
        is_input_available,
    )?;
    std::process::exit(code);
}
