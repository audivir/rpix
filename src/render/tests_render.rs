use super::*;
use image::{GenericImageView, Rgba};
use rstest::rstest;

const SVG_DATA: &[u8] = include_bytes!("../../fixtures/test.svg");
const PDF_DATA: &[u8] = include_bytes!("../../fixtures/test.pdf");
const HTML_DATA: &[u8] = include_bytes!("../../fixtures/test.html");
const RANDOM_DATA: &[u8] = include_bytes!("../../fixtures/test.random");

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
#[case(RANDOM_DATA)] // non-utf-8
fn test_render_html_chrome_invalid(#[case] html_data: &[u8]) {
    let result = render_html_chrome(html_data);
    assert!(result.is_err(), "HTML generation should fail");
}
