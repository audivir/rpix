# rpix

A image viewer for the Kitty Terminal Graphics Protocol.

**rpix** is a spiritual successor to `tpix`, rewritten in Rust with:

- wider SVG support using `resvg`,
- 16-bit PNG support,
- and PDF support using `pdfium`.

## Installation

### From Source

Ensure you have Rust installed.

```bash
git clone https://github.com/audivir/rpix
cd rpix
cargo build --release
cp target/release/rpix ~/.local/bin/
```

For PDF support, download `libpdfium.dylib` or `libpdfium.so` from [pdfium](https://github.com/bblanchon/pdfium-binaries/releases) and copy it in the same directory as `rpix`, one of the system library paths, or add the directory containing `libpdfium` library to `DYLD_LIBRARY_PATH` on macOS or `LD_LIBRARY_PATH` on Linux.

## Usage

```bash
# view single image
rpix image.png

# view multiple images
rpix image1.png image2.jpg logo.svg

# pipe from stdin
cat photo.webp | rpix

# resize to specific width
rpix -w 500 image.png

# force full terminal width
rpix -f image.png
```

### Options

| Flag                 | Description                                                       |
| -------------------- | ----------------------------------------------------------------- |
| `-w`, `--width`      | Specify image width in pixels.                                    |
| `-H`, `--height`     | Specify image height in pixels.                                   |
| `-f`, `--fullwidth`  | Resize image to fill the terminal width.                          |
| `-F`, `--fullheight` | Resize image to fill the terminal height.                         |
| `-r`, `--resize`     | Resize image to fill the terminal.                                |
| `-n`, `--noresize`   | Disable automatic resizing (show original size).                  |
| `-b`, `--background` | Add a background (useful for transparent PNGs/SVGs).              |
| `-C`, `--color`      | Set background color. Default: white.                             |
| `-m`, `--mode`       | Set transmission mode (png, zlib, raw). Default: png.             |
| `-i`, `--input`      | Set input type (auto, image, svg, pdf). Default: auto.            |
| `-P`, `--pages`      | Select pages to render (e.g. "1-3,34"), forces input type to pdf. |
| `-p`, `--printname`  | Print the filename before the image.                              |
| `-t`, `--tty`        | Force tty (ignore stdin check).                                   |
| `-c`, `--clear`      | Clear the terminal (remove all images).                           |

## License

MIT License. See [LICENSE](LICENSE) for details.

## Acknowledgments

- Based on the logic of [tpix](https://github.com/jesvedberg/tpix) by Jesper Svedberg (MIT License).
- Uses [resvg](https://github.com/RazrFalcon/resvg) for SVG rendering (MIT License).
- Uses pre-compiled [pdfium-binaries](https://github.com/bblanchon/pdfium-binaries/releases) for PDF rendering (MIT License).
- "fixtures/semi_transparent.png" is by Nguyễn Trí Minh Hoàng and is licensed under CC BY-SA 3.0.
