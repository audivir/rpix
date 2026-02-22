// implementation tests
use super::*;
use rstest::rstest;
use std::io::Cursor;

const NO_FILES_MSG: &str = "Error: No input files provided and no data piped to stdin.\n";
const SVG_DATA: &[u8] = include_bytes!("../fixtures/test.svg");

// dummy config
fn default_conf() -> Config {
    Config {
        files: vec![],
        width: None,
        height: None,
        fullwidth: false,
        fullheight: false,
        resize: false,
        noresize: false,
        background: false,
        color: "#FFFFFF".to_string(),
        mode: ModeOption::Png,
        output: None,
        overwrite: false,
        input: InputTypeOption::Auto,
        pages: "1".to_string(),
        all: false,
        language: None,
        no_newline: false,
        no_cache: false,
        printname: true, // default to true for tests
        tty: false,
        remove: false,
    }
}

fn run_test(
    conf: Config,
    is_input_available: bool,
    input: Cursor<&[u8]>,
    expected_output: &str,
    expected_error: &str,
    expected_code: i32,
    contains: bool,
    term_size: (u32, u32),
    cache_dir: Option<PathBuf>,
) {
    let mut output = Vec::new();
    let mut error_output = Vec::new();
    let code = run(
        &mut output,
        &mut error_output,
        input,
        conf,
        term_size,
        is_input_available,
        cache_dir,
    )
    .unwrap();
    let output_str = String::from_utf8(output).unwrap();
    let error_str = String::from_utf8(error_output).unwrap();
    assert_eq!(error_str, expected_error, "Error output mismatch");
    assert_eq!(code, expected_code, "Exit code mismatch");
    if contains {
        assert!(
            output_str.contains(expected_output),
            "Output should contain expected string",
        );
    } else {
        assert_eq!(output_str, expected_output, "Output mismatch");
    }
}

// --width, --height, --fullwidth, --fullheight, --resize, --noresize
#[rstest]
#[case(100, 50, Some(50), None, false, false, false, false, 50, 25)] // explicit width
#[case(100, 50, None, Some(25), false, false, false, false, 50, 25)] // explicit height
#[case(1000, 500, None, None, false, false, false, false, 100, 50)] // auto-downscale
#[case(50, 50, None, None, true, false, false, false, 100, 100)] // fullwidth
#[case(200, 25, None, None, false, true, false, false, 400, 50)] // fullheight
#[case(500, 500, None, None, false, false, true, false, 50, 50)] // resize (bound by height)
#[case(1000, 200, None, None, false, false, true, false, 100, 20)] // resize (bound by width)
#[case(1000, 500, None, None, false, false, false, true, 1000, 500)] // noresize
fn test_resize(
    #[case] orig_width: u32,
    #[case] orig_height: u32,
    #[case] width: Option<u32>,
    #[case] height: Option<u32>,
    #[case] fullwidth: bool,
    #[case] fullheight: bool,
    #[case] resize: bool,
    #[case] noresize: bool,
    #[case] expected_width: u32,
    #[case] expected_height: u32,
) {
    let svg_data = format!(
        "<svg width='{}' height='{}' xmlns='http://www.w3.org/2000/svg'><rect width='{}' height='{}' fill='red'/></svg>",
        orig_width, orig_height, orig_width, orig_height
    );
    let mut conf = default_conf();
    conf.mode = ModeOption::Raw; // to get width/height in output
    conf.width = width;
    conf.height = height;
    conf.fullwidth = fullwidth;
    conf.fullheight = fullheight;
    conf.resize = resize;
    conf.noresize = noresize;
    let expected_output = format!("\x1b_Ga=T,f=32,s={},v={}", expected_width, expected_height);
    run_test(
        conf,
        true,
        Cursor::new(&svg_data.into_bytes()),
        &expected_output,
        "stdin\n",
        0,
        true,
        (100, 50),
        None,
    );
}

// --background, --color
// TODO: implement

// --input
// TODO: implement

#[test]
fn test_output() {
    // get temp file path but do not create file
    let temp_file = tempfile::NamedTempFile::new().unwrap();
    let temp_file_path = temp_file.path().to_path_buf();

    let mut conf = default_conf();
    conf.files = vec!["fixtures/test.png".into()];
    conf.output = Some(temp_file_path.to_str().unwrap().to_string());

    let mut output = Vec::new();
    let mut error_output = Vec::new();
    let code = run(
        &mut output,
        &mut error_output,
        Cursor::new(&[]),
        conf,
        (800, 400),
        false,
        None,
    )
    .unwrap();
    assert_eq!(code, 0);
    let error_output_str = String::from_utf8(error_output).unwrap();
    assert_eq!(error_output_str, "fixtures/test.png\n");
    assert!(output.starts_with(b"\x89PNG"));
}

#[rstest]
#[case(vec![],"0", false, "Error: Invalid page range\n")]
#[case(vec![],"-1", false, "Error: Invalid page range\n")]
#[case(vec!["fixtures/test.pdf".into()],"2", false, "fixtures/test.pdf\nError loading fixtures/test.pdf: Page index out of range (must be <= 1)\n")]
#[case(vec!["fixtures/test.pdf".into(),"fixtures/test.png".into()],"1", false, "Error: Cannot specify multiple files with --pages\n")]
#[case(vec!["fixtures/test.pdf".into()],"1", true, "fixtures/test.pdf\n")]
fn test_pages(
    #[case] files: Vec<PathBuf>,
    #[case] pages: &str,
    #[case] success: bool,
    #[case] expected_error: &str,
) {
    let mut conf = default_conf();
    conf.files = files;
    conf.pages = pages.to_string();
    if success {
        run_test(
            conf,
            false,
            Cursor::new(&[]),
            "\x1b_Ga=T",
            expected_error,
            0,
            true,
            (800, 400),
            None,
        );
    } else {
        run_test(
            conf,
            false,
            Cursor::new(&[]),
            "",
            expected_error,
            1,
            false,
            (800, 400),
            None,
        );
    }
}

// --tty
#[rstest]
#[case(vec![], false, NO_FILES_MSG)]
#[case(vec!["fixtures/test.png".into()], true, "fixtures/test.png\n")]
fn test_force_tty(
    #[values(false, true)] is_input_available: bool,
    #[case] files: Vec<PathBuf>,
    #[case] success: bool,
    #[case] expected_error: &str,
) {
    let mut conf = default_conf();
    conf.files = files;
    conf.tty = true;
    if success {
        run_test(
            conf,
            is_input_available,
            Cursor::new(&[]),
            "\x1b_Ga=T",
            expected_error,
            0,
            true,
            (800, 400),
            None,
        );
    } else {
        run_test(
            conf,
            is_input_available,
            Cursor::new(&[]),
            "",
            expected_error,
            1,
            false,
            (800, 400),
            None,
        );
    }
}

// --printname
#[rstest]
fn test_printname(#[values(false, true)] printname: bool) {
    let mut conf = default_conf();
    conf.files = vec!["fixtures/test.png".into()];
    conf.printname = printname;
    let expected_error = if printname { "fixtures/test.png\n" } else { "" };
    run_test(
        conf,
        false,
        Cursor::new(&[]),
        "\x1b_Ga=T",
        expected_error,
        0,
        true,
        (800, 400),
        None,
    );
}

// --remove
#[rstest]
fn test_remove(
    #[values(false, true)] is_input_available: bool,
    #[values(false, true)] tty: bool,
    #[values(vec![], vec!["fixtures/test.png".into()])] files: Vec<PathBuf>,
) {
    let mut conf = default_conf();
    conf.remove = true;
    conf.tty = tty;
    conf.files = files;

    run_test(
        conf,
        is_input_available,
        Cursor::new(&[]),
        "\x1b_Ga=d\x1b\\",
        "",
        0,
        false,
        (800, 400),
        None,
    );
}

// [FILES]
#[rstest]
#[case(vec![])]
#[case(vec!["fixtures/test.png".into()])]
fn test_stdin(
    #[values("fixtures/test.svg".as_bytes(), SVG_DATA)] input_data: &[u8],
    #[case] files: Vec<PathBuf>,
) {
    let mut conf = default_conf();
    conf.files = files;
    run_test(
        conf,
        true,
        Cursor::new(input_data),
        "\x1b_Ga=T",
        "stdin\n",
        0,
        true,
        (800, 400),
        None,
    );
}

#[test]
fn test_no_input() {
    let conf = default_conf();
    run_test(
        conf,
        false,
        Cursor::new(&[]),
        "",
        NO_FILES_MSG,
        1,
        false,
        (800, 400),
        None,
    );
}

#[rstest]
#[case(vec!["fixtures/test.png".into()], "fixtures/test.png\n", 0)]
#[case(vec!["fixtures/test.jpg".into(), "fixtures/test.png".into()], "fixtures/test.jpg\nfixtures/test.png\n", 0)]
#[case(vec!["fixtures/test.png".into(), "nonexistent".into()], "fixtures/test.png\nnonexistent\nError loading nonexistent: Failed to open file\n", 1)]
fn test_files(
    #[case] files: Vec<PathBuf>,
    #[case] expected_error: &str,
    #[case] expected_code: i32,
) {
    let mut conf = default_conf();
    conf.files = files;
    run_test(
        conf,
        false,
        Cursor::new(&[]),
        "\x1b_Ga=T",
        expected_error,
        expected_code,
        true,
        (800, 400),
        None,
    );
}
