use std::env;
use std::path::PathBuf;

fn main() {
    let crate_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let package_name = env::var("CARGO_PKG_NAME").unwrap();
    let output_dir = target_dir().join(&package_name).join("include");

    std::fs::create_dir_all(&output_dir).expect("Failed to create output directory");

    let config = cbindgen::Config {
        language: cbindgen::Language::C,
        braces: cbindgen::Braces::SameLine,
        line_length: 100,
        tab_width: 4,
        documentation: true,
        documentation_style: cbindgen::DocumentationStyle::Doxy,
        ..Default::default()
    };

    cbindgen::Builder::new()
        .with_crate(crate_dir)
        .with_config(config)
        .generate()
        .expect("Unable to generate bindings")
        .write_to_file(output_dir.join("moq_ffi.h"));

    println!("cargo:rerun-if-changed=src/");
}

fn target_dir() -> PathBuf {
    if let Ok(target) = env::var("CARGO_TARGET_DIR") {
        PathBuf::from(target)
    } else {
        PathBuf::from(env::var("OUT_DIR").unwrap())
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf()
    }
}
