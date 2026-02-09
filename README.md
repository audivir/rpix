# kv

A image and document viewer for the Kitty Terminal Graphics Protocol.

**kv**, short for `Kitty Viewer`, is a spiritual successor to `tpix`, rewritten in Rust with:

- 16-bit PNG support,
- wider SVG support using `resvg`,
- PDF support using `pdfium`,
- HTML support using `headless_chrome`,
- Office support using `libreoffice` (and `pdfium` due to intermediate PDF representation, cached per default for performance),
- Text output using `bat`.

## Installation

### Prerequisites

- For PDF support, download `libpdfium.dylib` or `libpdfium.so` from [pdfium](https://github.com/bblanchon/pdfium-binaries/releases) and copy it in the same directory as `kv`, one of the system library paths, or add the directory containing `libpdfium` library to `DYLD_LIBRARY_PATH` on macOS or `LD_LIBRARY_PATH` on Linux.
- For HTML support, `headless_chrome` automatically downloads a chrome binary on the first run.
- For Office support, `soffice` (from `libreoffice`) and `libpdfium` are required.
  > Caveats: Office files are cached per default for performance. Use `-n` to disable caching.

### From Source

Ensure you have Rust installed.

```bash
git clone https://github.com/audivir/kv
cd kv
cargo build --release
cp target/release/kv ~/.local/bin/
```

## Usage

```bash
# view single image
kv image.png

# view multiple images
kv image1.png image2.jpg logo.svg

# pipe from stdin
cat photo.webp | kv

# resize to specific width
kv -w 500 image.png

# force full terminal width
kv -f image.png

# view specific pages of a pdf file
kv -P 1-3,34 pdf.pdf

# store a screenshot of an external domain as a png file
kv -o example.png https://example.org

# view office documents
kv document.docx
```

### Options

| Flag                 | Description                                                                           |
| -------------------- | ------------------------------------------------------------------------------------- |
| `-w`, `--width`      | Specify image width in pixels.                                                        |
| `-H`, `--height`     | Specify image height in pixels.                                                       |
| `-f`, `--fullwidth`  | Resize image to fill terminal width.                                                  |
| `-F`, `--fullheight` | Resize image to fill terminal height.                                                 |
| `-r`, `--resize`     | Resize image to fill terminal.                                                        |
| `-n`, `--noresize`   | Disable automatic resizing (show original size).                                      |
| `-b`, `--background` | Add a background (useful for transparent images).                                     |
| `-c`, `--color`      | Set background color as hex string. Default: #FFFFFF.                                 |
| `-m`, `--mode`       | Set transmission mode (png, zlib, raw). Default: png.                                 |
| `-o`, `--output`     | Output to file as png, instead of kitty.                                              |
| `-x`, `--overwrite`  | Overwrite existing output file.                                                       |
| `-i`, `--input`      | Set input type (auto, image, svg, pdf, html, office). Default: auto.                  |
| `-P`, `--pages`      | Select pages to render (e.g. "1-3,34" or empty for all). Default: 1.                  |
| `-A`, `--all`        | Select all pages.                                                                     |
| `-l`, `--language`   | Set language for syntax highlighting (e.g. "toml").                                   |
| `-N`, `--no-newline` | Do not add a newline after text data missing each input. (might mess up the terminal) |
| `-C`, `--no-cache`   | Do not cache office files.                                                            |
| `-p`, `--printname`  | Print the filename before image.                                                      |
| `-t`, `--tty`        | Force tty (ignore stdin check).                                                       |
| `-R`, `--remove`     | Remove all images from terminal.                                                      |

## License

MIT License. See [LICENSE](LICENSE) for details.

## Acknowledgments

- Based on the logic of [tpix](https://github.com/jesvedberg/tpix) by Jesper Svedberg (MIT License).
- Uses [resvg](https://github.com/RazrFalcon/resvg) for SVG rendering (MIT License).
- Uses [bat](https://github.com/sharkdp/bat) for text rendering (MIT License).
- Uses pre-compiled [pdfium-binaries](https://github.com/bblanchon/pdfium-binaries/releases) for PDF rendering (MIT License).
- "fixtures/semi_transparent.png" is by Nguyễn Trí Minh Hoàng and is licensed under CC BY-SA 3.0.
