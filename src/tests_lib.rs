// unit tests
use super::*;
use image::Rgba;
use image::{DynamicImage, GenericImageView};
use rstest::rstest;
use std::path::PathBuf;

const WHITE: Rgba<u8> = Rgba([255, 255, 255, 255]);
const BLACK: Rgba<u8> = Rgba([0, 0, 0, 255]);
const TRANSPARENT: Rgba<u8> = Rgba([0, 0, 0, 0]);

const PNG_DATA: &[u8] = include_bytes!("../fixtures/test.png");
const SVG_DATA: &[u8] = include_bytes!("../fixtures/test.svg");
const PDF_DATA: &[u8] = include_bytes!("../fixtures/test.pdf");
const HTML_DATA: &[u8] = include_bytes!("../fixtures/test.html");

fn default_ctx() -> RpixContext {
    RpixContext {
        input_type: InputType::Auto,
        conf_w: None,
        conf_h: None,
        term_width: 100,
        term_height: 50,
        page_indices: None,
    }
}
// get_term_size
// TODO: implement test

#[rstest]
#[case("FF0000", Rgba([255, 0, 0, 255]))]
#[case("00FF00", Rgba([0, 255, 0, 255]))]
#[case("0000FF", Rgba([0, 0, 255, 255]))]
#[case("FFFFFF", Rgba([255, 255, 255, 255]))]
#[case("000000", Rgba([0, 0, 0, 255]))]
fn test_parse_color(#[case] color: &str, #[case] expected: Rgba<u8>) {
    let result = parse_color(color);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), expected);
}

#[rstest]
#[case("FF00")]
#[case("FF000000")]
#[case("FF00GG")]
fn test_parse_color_invalid(#[case] color: &str) {
    let result = parse_color(color);
    assert!(result.is_err());
}

#[rstest]
#[case(WHITE, TRANSPARENT, WHITE)]
#[case(BLACK, TRANSPARENT, BLACK)]
#[case(WHITE, BLACK, BLACK)]
#[case(WHITE, Rgba([255, 0, 0, 128]), Rgba([255, 127, 127, 255]))]
#[case(BLACK, Rgba([255, 0, 0, 128]), Rgba([128, 0, 0, 255]))]
fn test_add_background(
    #[case] color: Rgba<u8>,
    #[case] src_pixel: Rgba<u8>,
    #[case] expected_pixel: Rgba<u8>,
) {
    let mut img = DynamicImage::new_rgba8(1, 1); // 1x1 pixel
    img.as_mut_rgba8().unwrap().put_pixel(0, 0, src_pixel); // black, 100% alpha

    img = add_background(&img, &color);

    let pixel = img.get_pixel(0, 0);
    assert_eq!(
        pixel, expected_pixel,
        "Background color not applied correctly"
    );
}

#[rstest]
#[case(100, 50, Some(50), None, false, false, false, false, 50, 25)] // explicit width
#[case(100, 50, None, Some(25), false, false, false, false, 50, 25)] // explicit height
#[case(1000, 500, None, None, false, false, false, false, 100, 50)] // auto-downscale
#[case(50, 50, None, None, true, false, false, false, 100, 100)] // fullwidth
#[case(200, 25, None, None, false, true, false, false, 400, 50)] // fullheight
#[case(500, 500, None, None, false, false, true, false, 50, 50)] // resize (bound by height)
#[case(1000, 200, None, None, false, false, true, false, 100, 20)] // resize (bound by width)
#[case(1000, 500, None, None, false, false, false, true, 1000, 500)] // noresize
fn test_calculate_dimensions(
    #[case] img_w: u32,
    #[case] img_h: u32,
    #[case] conf_w: Option<u32>,
    #[case] conf_h: Option<u32>,
    #[case] fullwidth: bool,
    #[case] fullheight: bool,
    #[case] resize: bool,
    #[case] noresize: bool,
    #[case] expected_w: u32,
    #[case] expected_h: u32,
) {
    let img_dims = (img_w, img_h);
    let term_width = 100; // fixed
    let term_height = 50; // fixed

    // quick sanity check for exclusive options
    assert!(!(fullwidth && fullheight));
    assert!(!(resize && noresize));
    assert!(!((resize || noresize) && (fullwidth || fullheight)));
    assert!(
        !((conf_w.is_some() || conf_h.is_some())
            && (fullwidth || fullheight || resize || noresize))
    );

    let (w, h) = calculate_dimensions(
        img_dims,
        (conf_w, conf_h),
        fullwidth,
        fullheight,
        resize,
        noresize,
        (term_width, term_height),
    );

    assert_eq!(w, expected_w);
    assert_eq!(h, expected_h);
}

#[rstest]
#[case("1", vec![0])]
#[case("1,1", vec![0])]
#[case("1,2", vec![0, 1])]
#[case("2,1", vec![0, 1])]
#[case("2,,3", vec![1, 2])]
#[case("1-3", vec![0, 1, 2])]
#[case("1-3,5", vec![0, 1, 2, 4])]
#[case("1-3,5-7", vec![0, 1, 2, 4, 5, 6])]
fn test_parse_pages(#[case] input: &str, #[case] expected: Vec<u16>) {
    let result = parse_pages(input);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Some(expected));
}

#[test]
fn test_parse_pages_empty() {
    let result = parse_pages("");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), None);
}

#[rstest]
#[case("a")]
#[case("0")]
#[case("-1")]
#[case("1-2,4-3")]
fn test_parse_pages_invalid(#[case] input: &str) {
    let result = parse_pages(input);
    assert!(result.is_err());
}

#[test]
fn test_render_svg() {
    let result = render_svg(SVG_DATA);
    assert!(result.is_ok(), "SVG generation failed");

    let img = result.unwrap();
    assert_eq!(img.width(), 1);
    assert_eq!(img.height(), 1);

    let pixel = img.get_pixel(0, 0);
    assert_eq!(pixel, Rgba([102, 102, 102, 255]));
}

#[test]
fn test_render_svg_invalid() {
    let svg_data = br#"<svg>invalid"#;

    let result = render_svg(svg_data);
    assert!(result.is_err(), "SVG generation failed");
}

#[rstest]
#[case(None, 100, None, 100)]
#[case(None, 100, Some(vec![0]), 100)]
#[case(Some(10), 100, None, 10)]
fn test_render_pdf(
    #[case] conf_w: Option<u32>,
    #[case] term_width: u32,
    #[case] page_indices: Option<Vec<u16>>,
    #[case] expected_width: u32,
) {
    let result = render_pdf(PDF_DATA, conf_w, term_width, page_indices);
    assert!(result.is_ok(), "PDF generation failed");

    let img = result.unwrap();
    assert_eq!(img.width(), expected_width);

    let pixel = img.get_pixel(0, 0);
    assert_eq!(pixel, Rgba([255, 255, 255, 255]));
}

#[test]
fn test_render_pdf_invalid() {
    let pdf_data = br#"%PDF-1.4
invalid"#;

    let result = render_pdf(pdf_data, None, 100, None);
    assert!(result.is_err(), "PDF generation failed");
}

#[rstest]
#[case(vec![])]
#[case(vec![2])]
fn test_render_pdf_out_of_range(#[case] page_indices: Vec<u16>) {
    let result = render_pdf(PDF_DATA, None, 100, Some(page_indices));
    assert!(result.is_err(), "PDF generation failed");
}

#[rstest]
#[case(HTML_DATA)]
#[case(b"fixtures/test.html")]
#[case(b"https://commons.wikimedia.org/wiki/File:Solid_red.png")]
fn test_render_html_chrome(#[case] html_data: &[u8]) {
    let result = render_html_chrome(html_data);
    assert!(result.is_ok(), "HTML generation failed");

    let img = result.unwrap();

    // iterate through all pixels and check if any is red
    let mut red_found = false;
    for x in 0..img.width() {
        for y in 0..img.height() {
            let pixel = img.get_pixel(x, y);
            if pixel == Rgba([255, 0, 0, 255]) {
                red_found = true;
                break;
            }
        }
    }
    assert!(red_found, "Red pixel not found");
}

#[rstest]
#[case(b"\x76\xcf")] // non-utf-8
fn test_render_html_chrome_invalid(#[case] html_data: &[u8]) {
    let result = render_html_chrome(html_data);
    assert!(result.is_err(), "HTML generation should fail");
}

#[rstest]
#[case(PathBuf::from("fixtures/test.svg"), InputType::Svg)]
#[case(PathBuf::from("fixtures/test.png"), InputType::Image)]
#[case(PathBuf::from("fixtures/test.pdf"), InputType::Pdf)]
fn test_load_file(#[case] path: PathBuf, #[case] input_type: InputType) {
    let mut ctx = default_ctx();
    ctx.input_type = input_type;
    let result = load_file(&ctx, &path);
    assert!(result.is_ok());
    let result_auto = load_file(&ctx, &path);
    assert!(result_auto.is_ok());
}

#[rstest]
#[case(PathBuf::from("nonexistent"), InputType::Auto, "Failed to open file")]
#[case(
    PathBuf::from("fixtures/test.random"),
    InputType::Auto,
    "Failed to decode input: The image format could not be determined"
)]
#[case(
    PathBuf::from("fixtures/test.svg"),
    InputType::Image,
    "Failed to load image"
)]
#[case(
    PathBuf::from("fixtures/test.png"),
    InputType::Svg,
    "Failed to parse SVG"
)]
#[case(
    PathBuf::from("fixtures/test.pdf"),
    InputType::Svg,
    "Failed to parse SVG"
)]
fn test_load_file_invalid(
    #[case] path: PathBuf,
    #[case] input_type: InputType,
    #[case] err_msg: &str,
) {
    let mut ctx = default_ctx();
    ctx.input_type = input_type;
    let result = load_file(&ctx, &path);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().to_string(), err_msg);
}

#[rstest]
#[case("fixtures/test.svg".as_bytes())]
#[case(SVG_DATA)]
#[case("fixtures/test.png".as_bytes())]
#[case(PNG_DATA)]
fn test_load_data(#[case] data: &[u8]) {
    let ctx = default_ctx();
    let result = load_data(&ctx, data, "");
    assert!(result.is_ok());
}

#[rstest]
#[case("nonexistent".as_bytes(), Some("Failed to decode input: The image format could not be determined"))]
#[case("invalidbinary\x00\x01\x02\x03".as_bytes(), Some("Failed to decode input: The image format could not be determined"))]
#[case("<svg>invalid".as_bytes(), Some("Failed to parse SVG"))]
#[case(
    b"",
    Some("Failed to decode input: The image format could not be determined")
)]
fn test_load_data_invalid(#[case] data: &[u8], #[case] err_msg: Option<&str>) {
    let ctx = default_ctx();
    let result = load_data(&ctx, data, "");
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().to_string(), err_msg.unwrap());
}
