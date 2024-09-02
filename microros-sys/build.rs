use std::env;
use std::path::PathBuf;

fn main() {
    let lib_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("../micro_ros_raspberrypi_pico_sdk/libmicroros/");

    println!("cargo:rustc-link-search={}", lib_dir.to_str().unwrap());
    println!(
        "cargo:rustc-link-search={}",
        "/usr/lib/arm-none-eabi/lib/thumb/v6-m/nofp/"
    );
    println!(
        "cargo:rustc-link-search={}",
        "/usr/lib/gcc/arm-none-eabi/10.3.1/thumb/v6-m/nofp/"
    );
    println!("cargo:rustc-link-lib=microros");
    println!("cargo:rustc-link-lib=nosys");
    println!("cargo:rustc-link-lib=c");
    println!("cargo:rustc-link-lib=m");
    println!("cargo:rustc-link-lib=g");
    println!("cargo:rustc-link-lib=stdc++");
    println!("cargo:rustc-link-lib=gcc");

    let bindings = bindgen::Builder::default()
        .header("wrapper.h")
        .use_core()
        .clang_arg(format!("-I{}", lib_dir.join("include").to_str().unwrap()))
        .clang_arg(format!("-I{}", "/usr/lib/arm-none-eabi/include/"))
        .clang_arg(format!("-I{}", "/usr/lib/gcc/arm-none-eabi/10.3.1/include/"))
        .generate()
        .expect("generation failed");
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("writing failed");
}
