use cc;
fn main() {
    // Specify the C library name and path
    let library_name = "testrustinput.a";
    // let build_command = if cfg!(target_os = "macos") {
    //     "make"
    // } else {
    //     "cmake"
    // };

    // let _ = std::process::Command::new(build_command)
    //     .current_dir(library_path)
    //     .status();
    cc::Build::new()
        .files(&["src/groestl/custom_groestl/groestl256/hash.c", "src/groestl/custom_groestl/groestl256/bs.c" ]) // Include both C files
        .include("src/groestl/custom_groestl/groestl256") // Include the directory containing the headers
        .compile(library_name); // Compile into a static library

    let library_path = env!("CARGO_MANIFEST_DIR");
    // Print the library path
    println!("XXXXXXXXXXXXXXXXXXXXX Library Path:");

    println!("XXXXXXXXXXXXXXXXXXXXX Library Path: {}", library_path);

    // // Link the C library
    println!("cargo:rustc-link-lib=static={}", library_name);
    println!("cargo:rustc-link-search={}", library_path);

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