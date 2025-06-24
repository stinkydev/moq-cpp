use std::env;
use std::path::PathBuf;

fn main() {
    // Tell Cargo about Windows system libraries needed by dependencies
    #[cfg(target_os = "windows")]
    {
        println!("cargo:rustc-link-lib=crypt32");
        println!("cargo:rustc-link-lib=secur32");
        println!("cargo:rustc-link-lib=ncrypt");
        println!("cargo:rustc-link-lib=kernel32");
        println!("cargo:rustc-link-lib=advapi32");
        println!("cargo:rustc-link-lib=ws2_32");
        println!("cargo:rustc-link-lib=userenv");
        println!("cargo:rustc-link-lib=bcrypt");
    }

    // Always try to generate header with cbindgen
    if let Err(e) = generate_cbindgen_header() {
        eprintln!("cbindgen failed: {}", e);
        eprintln!("This indicates a problem with the FFI definitions or cbindgen configuration.");
        eprintln!("Please check that all exported functions have proper #[no_mangle] and 'extern \"C\"' declarations.");
        panic!("cbindgen header generation failed");
    }
    
    println!("cargo:rerun-if-changed=cbindgen.toml");
    println!("cargo:rerun-if-changed=src/lib.rs");
}

fn generate_cbindgen_header() -> Result<(), Box<dyn std::error::Error>> {
    let crate_dir = env::var("CARGO_MANIFEST_DIR")?;
    let package_name = env::var("CARGO_PKG_NAME")?;
    let output_dir = target_dir().join(&package_name).join("include");
    
    // Also put header in source directory for development
    let source_include_dir = PathBuf::from(&crate_dir).join("include");

    std::fs::create_dir_all(&output_dir)?;
    std::fs::create_dir_all(&source_include_dir)?;

    let config = cbindgen::Config::from_file("cbindgen.toml")?;
    
    let header = cbindgen::Builder::new()
        .with_crate(crate_dir)
        .with_config(config)
        .generate()?;
    
    // Write to both target directory and source directory
    header.write_to_file(output_dir.join("moq_ffi.h"));
    header.write_to_file(source_include_dir.join("moq_ffi.h"));

    println!("Successfully generated moq_ffi.h with cbindgen");
    Ok(())
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
