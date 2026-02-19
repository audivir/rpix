use anyhow::{Context, Result};
use base64::{engine::general_purpose, Engine as _};
use bat::{Input, PrettyPrinter};
use flate2::write::ZlibEncoder;
use flate2::Compression;
use image::codecs::png::PngEncoder;
use image::{DynamicImage, GenericImageView, ImageEncoder};
use std::io::{Cursor, Write};
use std::path::PathBuf;

const KITTY_CHUNK_SIZE: usize = 4096;
const INPUT_CHUNK_SIZE: usize = (KITTY_CHUNK_SIZE * 3) / 4;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mode {
    Png,
    Zlib,
    Raw,
}

pub enum PrinterInput {
    File(PathBuf),
    Data(Vec<u8>),
}

pub fn send_image(
    writer: &mut dyn Write,
    img: DynamicImage,
    output: Option<String>,
    mode: Mode,
) -> Result<()> {
    let payload = match mode {
        Mode::Png => {
            let mut buffer = Vec::new();
            let (width, height) = img.dimensions();
            let color_type = img.color();

            // scope ensures flush
            {
                let encoder = PngEncoder::new(&mut buffer);
                encoder
                    .write_image(img.as_bytes(), width, height, color_type.into())
                    .context("Failed to encode image to PNG")?;
            }
            buffer
        }
        Mode::Raw => img.to_rgba8().into_raw(),
        Mode::Zlib => {
            let raw_bytes = img.to_rgba8().into_raw();
            let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
            encoder.write_all(&raw_bytes)?;
            encoder.finish().context("Failed to compress image data")?
        }
    };

    if output.is_some() {
        writer.write_all(&payload)?;
        return Ok(());
    }
    // Png: a=T,f=100
    // Raw: a=T,f=32,s={w},v={h} (+ o=z if zlib)
    let (width, height) = img.dimensions();
    let header = match mode {
        Mode::Png => "a=T,f=100".to_string(),
        Mode::Zlib => format!("a=T,f=32,s={},v={},o=z", width, height),
        Mode::Raw => format!("a=T,f=32,s={},v={}", width, height),
    };

    let total_len = payload.len();
    let mut offset = 0;

    // reusable buffer
    let mut b64_buffer = String::with_capacity(KITTY_CHUNK_SIZE + 4);

    while offset < total_len {
        let end = (offset + INPUT_CHUNK_SIZE).min(total_len);
        let chunk_data = &payload[offset..end];

        // encode chunk to base64
        b64_buffer.clear();
        general_purpose::STANDARD.encode_string(chunk_data, &mut b64_buffer);

        let more = if end < total_len { 1 } else { 0 };

        write!(writer, "\x1b_G")?;

        // send control header only on the first chunk
        if offset == 0 {
            write!(writer, "{},", header)?;
        }

        // send payload
        write!(writer, "m={};", more)?;
        writer.write_all(b64_buffer.as_bytes())?;

        // end escape sequence
        write!(writer, "\x1b\\")?;

        offset = end;
    }

    // ensure terminal is clean
    writeln!(writer)?;
    writer.flush()?;

    Ok(())
}

pub fn pretty_print(
    writer: &mut dyn Write,
    input: PrinterInput,
    language: Option<&str>,
    newline: bool,
) -> Result<()> {
    let mut printer = PrettyPrinter::new();

    match input {
        PrinterInput::File(path) => {
            printer.input_file(path);
        }
        PrinterInput::Data(data) => {
            // requires a reader
            printer.input(Input::from_reader(Box::new(Cursor::new(data))));
        }
    };

    if let Some(lang) = language {
        printer.language(lang);
    }

    // capture output to handle the optional newline
    let mut output_str = String::new();
    printer
        .print_with_writer(Some(&mut output_str))
        .map_err(|e| anyhow::anyhow!("Syntax highlighting failed: {}", e))?;

    if newline && !output_str.ends_with('\n') {
        output_str.push('\n');
    }

    writer.write_all(output_str.as_bytes())?;
    writer.flush()?;
    Ok(())
}
