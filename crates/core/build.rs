use cc;
fn main() {
    // Specify the C library name and path
    let library_name = "bs_mul_128";
    let files = ["src/bitsliced_mul/bs_multiply_128.c", "src/bitsliced_mul/bs.c"];
    let includes = "src/bitsliced_mul";

    if cfg!(target_os = "windows") {
        cc::Build::new()
        .files(files) // Include both C files
        // .flag("/O2")
        // .flag("/ftree-vectorize")
        .include(includes) // Include the directory containing the headers
        .compile(library_name); // Compile into a static library
    } else {
        cc::Build::new()
        .files(files) // Include both C files
        .flag_if_supported("-O3")
        .flag_if_supported("-ftree-vectorize")
        .flag_if_supported("-Wall")
        .flag("-O2")
        // .flag("-ftree-vectorize")
        .include(includes) // Include the directory containing the headers
        .compile(library_name); // Compile into a static library
    }
    
    let library_path = env!("CARGO_MANIFEST_DIR");
    // Print the library path
    println!("XXXXXXXXXXXXXXXXXXXXX Library Path:");

    println!("XXXXXXXXXXXXXXXXXXXXX Library Path: {}", library_path);

    // // Link the C library
    println!("cargo:rustc-link-lib=static={}", library_name);
    println!("cargo:rustc-link-search={}", library_path);
    println!("cargo:rerun-if-changed=build.rs");


    // Build the C library using make or cmake
    // #[cfg(target_os = "macos")]
    // {
    //     let _ = std::process::Command::new("make")
    //         .current_dir(library_path)
    //         .status();
    // }

    // #[cfg(target_os = "linux")]
    // {
    //     let _ = std::process::Command::new("cmake")
    //         .arg("--build")
    //         .arg(library_path)
    //         .status();
    // }
}