//! What the installer carries: the NSIS installer it runs (`PRISME_SETUP_PAYLOAD`, built by the
//! release before it; none in a development build, which only plays the installation), and its
//! page with the font and the prism inside, as it shows before anything is installed.

use std::path::Path;

const PAYLOAD: &str = "PRISME_SETUP_PAYLOAD";
const FONT: &str = "../../frontend/src/styles/fonts/instrument-sans-latin-wdth-normal.woff2";
const LOGO: &str = "../../design/logo-mark.svg";

fn main() {
    let out = std::env::var("OUT_DIR").expect("set by cargo");
    let out = Path::new(&out);
    println!("cargo:rerun-if-env-changed={PAYLOAD}");
    let payload = match std::env::var(PAYLOAD) {
        Ok(path) => {
            println!("cargo:rerun-if-changed={path}");
            std::fs::read(&path).unwrap_or_else(|e| panic!("{PAYLOAD} {path}: {e}"))
        }
        Err(_) => Vec::new(),
    };
    std::fs::write(out.join("payload.exe"), payload).expect("payload written");

    for file in ["src/page.html", FONT, LOGO] {
        println!("cargo:rerun-if-changed={file}");
    }
    let page = std::fs::read_to_string("src/page.html")
        .expect("the page")
        .replace("__FONT__", &base64(&std::fs::read(FONT).expect("the font")))
        .replace("__LOGO__", &std::fs::read_to_string(LOGO).expect("the prism"));
    std::fs::write(out.join("page.html"), page).expect("page written");

    #[cfg(windows)]
    {
        let mut resources = tauri_winres::WindowsResource::new();
        resources.set_icon("../prisme-desktop/icons/icon.ico");
        resources.set("ProductName", "Prisme");
        resources.set("FileDescription", "Prisme Setup");
        resources.compile().expect("the installer's icon");
    }
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut text = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, &b)| n | u32::from(b) << (16 - 8 * i));
        for i in 0..4 {
            text.push(if i <= chunk.len() {
                ALPHABET[(n >> (18 - 6 * i)) as usize & 63] as char
            } else {
                '='
            });
        }
    }
    text
}
