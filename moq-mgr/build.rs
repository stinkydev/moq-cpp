use std::path::PathBuf;

fn main() {
    let crate_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let package_name = std::env::var("CARGO_PKG_NAME").unwrap();
    let output_file = format!("{}.h", package_name);

    cbindgen::Builder::new()
        .with_crate(crate_dir)
        .with_language(cbindgen::Language::C)
        .with_cpp_compat(true)
        .generate()
        .expect("Unable to generate bindings")
        .write_to_file(PathBuf::from(&output_file));

    println!("cargo:rerun-if-changed=src/");
}
