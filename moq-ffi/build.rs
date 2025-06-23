use std::env;
use std::path::PathBuf;

fn main() {
    let package_name = env::var("CARGO_PKG_NAME").unwrap();
    let output_dir = target_dir().join(&package_name).join("include");

    std::fs::create_dir_all(&output_dir).expect("Failed to create output directory");

    // Copy the manual header instead of using cbindgen
    let manual_header_src = PathBuf::from("include/moq_ffi_manual.h");
    let manual_header_dst = output_dir.join("moq_ffi.h");
    
    println!("Copying header from {:?} to {:?}", manual_header_src, manual_header_dst);
    std::fs::copy(&manual_header_src, &manual_header_dst).expect("Failed to copy manual header");
    
    // Verify the file was copied
    if manual_header_dst.exists() {
        println!("Header successfully copied to {:?}", manual_header_dst);
    } else {
        panic!("Header file was not created at {:?}", manual_header_dst);
    }

    println!("cargo:rerun-if-changed=include/moq_ffi_manual.h");
}

fn target_dir() -> PathBuf {
    if let Ok(target) = env::var("CARGO_TARGET_DIR") {
        PathBuf::from(target).join("release")
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
