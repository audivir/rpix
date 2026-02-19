use crate::{pretty_print, send_image};
use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use kv::*;
use std::io::{self, BufWriter, Read, Write};
use std::path::PathBuf;
use tempfile::NamedTempFile;

#[cfg(test)]
mod tests_main;

#[derive(Debug, Clone, ValueEnum, PartialEq)]
enum ModeOption {
    Png,
    Zlib,
    Raw,
}

impl From<ModeOption> for Mode {
    fn from(arg: ModeOption) -> Self {
        match arg {
            ModeOption::Png => Mode::Png,
            ModeOption::Zlib => Mode::Zlib,
            ModeOption::Raw => Mode::Raw,
        }
    }
}

#[derive(Debug, Clone, ValueEnum, PartialEq)]
enum InputTypeOption {
    Auto,
    Image,
    Text,
    Svg,
    Pdf,
    Html,
    Office,
}

impl From<InputTypeOption> for InputType {
    fn from(arg: InputTypeOption) -> Self {
        match arg {
            InputTypeOption::Auto => InputType::Auto,
            InputTypeOption::Image => InputType::Image,
            InputTypeOption::Text => InputType::Text,
            InputTypeOption::Svg => InputType::Svg,
            InputTypeOption::Pdf => InputType::Pdf,
            InputTypeOption::Html => InputType::Html,
            InputTypeOption::Office => InputType::Office,
        }
    }
}

type TempAndFinalOption = Option<(NamedTempFile, PathBuf)>;

/// A image viewer for the Kitty Terminal Graphics Protocol.
#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
struct Config {
    /// Input files
    #[arg(name = "FILES")]
    files: Vec<PathBuf>,

    /// Specify image width in pixels
    #[arg(
        short = 'w',
        long,
        conflicts_with_all = ["height", "fullwidth", "fullheight", "resize", "noresize"],
    )]
    width: Option<u32>,

    /// Specify image height in pixels
    #[arg(
        short = 'H', // else conflicts with --help
        long,
        conflicts_with_all = ["width", "fullwidth", "fullheight", "resize", "noresize"],
    )]
    height: Option<u32>,

    /// Resize image to fill terminal width
    #[arg(
        short = 'f',
        long,
        conflicts_with_all = ["width", "height", "fullheight", "resize", "noresize"],
    )]
    fullwidth: bool,

    /// Resize image to fill terminal height
    #[arg(
        short = 'F',
        long,
        conflicts_with_all = ["width", "height", "fullwidth", "resize", "noresize"],
    )]
    fullheight: bool,

    /// Resize image to fill terminal
    #[arg(
        short = 'r',
        long,
        conflicts_with_all = ["width", "height", "fullwidth", "fullheight", "noresize"],
    )]
    resize: bool,

    /// Disable automatic resizing (show original size)
    #[arg(
        short = 'n',
        long,
        conflicts_with_all = ["width", "height", "fullwidth", "fullheight", "resize"],
    )]
    noresize: bool,

    /// Add background (useful for transparent images)
    #[arg(short = 'b', long)]
    background: bool,

    /// Set background color as hex string
    #[arg(short = 'c', long, default_value = "#FFFFFF", requires = "background")]
    color: String,

    /// Set transmission mode
    #[arg(short = 'm', long, value_enum, default_value_t = ModeOption::Png)]
    mode: ModeOption,

    /// Output to file as png, instead of kitty
    #[arg(short = 'o', long, conflicts_with = "mode")]
    output: Option<String>,

    /// Overwrite existing output file
    #[arg(short = 'x', long, requires = "output")]
    overwrite: bool,

    /// Set input type
    #[arg(short = 'i', long, value_enum, default_value_t = InputTypeOption::Auto)]
    input: InputTypeOption,

    /// Select pages to render (e.g. "1-3,34" or empty for all)
    #[arg(short = 'P', long, default_value = "1", conflicts_with = "all")]
    pages: String,

    /// Select all pages
    #[arg(short = 'A', long, conflicts_with = "pages")]
    all: bool,

    /// Set language for syntax highlighting (e.g. "toml")
    #[arg(short = 'l', long)]
    language: Option<String>,

    /// Do not add a newline after each input (might mess up the terminal)
    #[arg(short = 'N', long)]
    no_newline: bool,

    /// Do not cache office files
    #[arg(short = 'C', long)]
    no_cache: bool,

    /// Print filename before each input
    #[arg(short = 'p', long)]
    printname: bool,

    /// Force tty (ignore stdin check)
    #[arg(short = 't', long)]
    tty: bool,

    /// Remove all images from terminal
    #[arg(short = 'R', long, conflicts_with = "plugins")]
    remove: bool,

    /// Print the plugins configuration file path (will be created if it doesn't exist)
    #[arg(long, conflicts_with = "remove")]
    plugins: bool,
}

fn run(
    mut writer: impl Write,
    mut err_writer: impl Write,
    mut reader: impl Read,
    conf: Config,
    term_size: (u32, u32),
    is_input_available: bool,
    cache_dir: Option<PathBuf>,
) -> Result<i32> {
    if conf.remove {
        write!(writer, "\x1b_Ga=d\x1b\\")?;
        return Ok(0);
    }

    // If -t is passed, we ignore stdin even if input is available
    let use_stdin = is_input_available && !conf.tty;

    if conf.output.is_some() && !use_stdin && conf.files.len() > 1 {
        writeln!(
            err_writer,
            "Error: Cannot specify multiple files with --output"
        )?;
        return Ok(1);
    }

    let page_indices = if conf.all {
        if !use_stdin && conf.files.len() > 1 {
            writeln!(
                err_writer,
                "Error: Cannot specify multiple files with --all"
            )?;
            return Ok(1);
        }
        None
    } else if let Ok(pages) = parse_pages(&conf.pages) {
        // if pages != [0], disallow multiple files
        if !use_stdin && conf.files.len() > 1 && pages != Some(vec![0]) {
            writeln!(
                err_writer,
                "Error: Cannot specify multiple files with non-default --pages option"
            )?;
            return Ok(1);
        }
        pages
    } else {
        writeln!(err_writer, "Error: Invalid page range")?;
        return Ok(1);
    };

    let resize_mode = if conf.noresize {
        ResizeMode::Original
    } else if conf.resize {
        ResizeMode::FitTerminal
    } else if conf.fullwidth {
        ResizeMode::FitWidth
    } else if conf.fullheight {
        ResizeMode::FitHeight
    } else if conf.width.is_some() || conf.height.is_some() {
        ResizeMode::Manual {
            width: conf.width,
            height: conf.height,
        }
    } else {
        ResizeMode::ClipTerminal
    };

    let cache_mode = if conf.no_cache {
        CacheMode::Disabled
    } else if let Some(cache_dir) = cache_dir {
        CacheMode::Custom(cache_dir)
    } else {
        CacheMode::Default
    };

    let background_color = if conf.background {
        Some(parse_color(&conf.color)?)
    } else {
        None
    };

    let ctx = KvContext {
        input_type: conf.input.clone().into(),
        resize_mode,
        term_size,
        page_indices,
        cache_mode,
        background_color,
    };

    if use_stdin {
        if conf.printname {
            writeln!(err_writer, "stdin")?;
        }

        let mut data = Vec::new();
        reader.read_to_end(&mut data)?;

        match load_data(&ctx, &data, "") {
            Ok(LoadResult::Image(img)) => {
                send_image(
                    &mut writer,
                    img,
                    conf.output.clone(),
                    conf.mode.clone().into(),
                )?;
            }
            Ok(LoadResult::Data(data)) => {
                pretty_print(
                    &mut writer,
                    PrinterInput::Data(data),
                    conf.language.as_deref(),
                    !conf.no_newline,
                )?;
            }
            Err(e) => {
                writeln!(err_writer, "Error decoding stdin: {}", e)?;
                return Ok(1);
            }
        }
    } else if !conf.files.is_empty() {
        let mut exit_code = 0;
        for path in &conf.files {
            if conf.printname {
                writeln!(err_writer, "{}", path.display())?;
            }
            match load_file(&ctx, path) {
                Ok(LoadResult::Image(img)) => {
                    send_image(
                        &mut writer,
                        img,
                        conf.output.clone(),
                        conf.mode.clone().into(),
                    )?;
                }
                Ok(LoadResult::Data(_)) => {
                    pretty_print(
                        &mut writer,
                        PrinterInput::File(path.clone()),
                        conf.language.as_deref(),
                        !conf.no_newline,
                    )?;
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

fn prepare_writer(
    output: Option<String>,
    overwrite: bool,
) -> Result<(Box<dyn Write>, TempAndFinalOption)> {
    match output {
        Some(path_str) => {
            let path = PathBuf::from(path_str);

            let absolute_path = if !path.is_absolute() {
                std::env::current_dir()?.join(&path)
            } else {
                path.clone()
            };

            let parent = absolute_path.parent().context("Invalid output path")?;

            if !parent.exists() {
                anyhow::bail!("Output directory does not exist: {}", parent.display());
            }

            if absolute_path.exists() && !overwrite {
                anyhow::bail!(
                    "Output file already exists: {} (use --overwrite)",
                    path.display()
                );
            }

            let tempfile = NamedTempFile::new_in(parent).context(format!(
                "Failed to create temp file in {}",
                parent.display()
            ))?;

            let file = tempfile
                .as_file()
                .try_clone()
                .context("Failed to clone temp file")?;

            let writer: Box<dyn Write> = Box::new(BufWriter::new(file));
            Ok((writer, Some((tempfile, absolute_path))))
        }

        None => Ok((Box::new(BufWriter::new(io::stdout())), None)),
    }
}

fn main() -> Result<()> {
    let conf = Config::parse();

    if conf.plugins {
        open_config()?;
        return Ok(());
    }

    let term_size = get_term_size();

    // Detect TTY status
    let is_input_available = atty::isnt(atty::Stream::Stdin);

    let (writer, temp_output) = prepare_writer(conf.output.clone(), conf.overwrite)?;

    let code = run(
        writer,
        io::stderr(),
        io::stdin(),
        conf,
        term_size,
        is_input_available,
        None,
    )?;

    // Commit temp file only on success
    if let Some((tempfile, final_path)) = temp_output {
        if code == 0 {
            tempfile.persist(final_path)?;
        }
    }

    std::process::exit(code);
}
