use std::path::PathBuf;

fn main() {
    let includes = if cfg!(windows) {
        let lib = vcpkg::Config::new().find_package("icu").unwrap();
        lib.include_paths
    } else if cfg!(target_os = "macos") {
        println!("cargo:rustc-link-search=native=/opt/homebrew/lib");
        vec![PathBuf::from("/opt/homebrew/include")]
    } else {
        vec![]
    };

    #[cfg(windows)]
    let dst = cmake::Config::new("cg3")
        .define("WIN32", "ON")
        .define("MSVC", "ON")
        .define("CMAKE_CXX_FLAGS", "/Dcg3_EXPORTS /DWIN32 /D_WIN32 /D_WINDOWS /W3 /GR /EHsc /O2")
        .define("BUILD_SHARED_LIBS", "OFF")
        .build();

    #[cfg(unix)]
    let dst = cmake::Config::new("cg3")
        .define("BUILD_SHARED_LIBS", "OFF")
        .build();

    println!("cargo:rustc-link-search=native={}/lib", dst.display());

    println!("cargo:rustc-link-lib=cg3");

    println!("cargo:rustc-link-lib=icuuc");
    println!("cargo:rustc-link-lib=icuio");
    if cfg!(windows) {
        println!("cargo:rustc-link-lib=icudt");
        println!("cargo:rustc-link-lib=icuin");
    } else {
        println!("cargo:rustc-link-lib=icudata");
        println!("cargo:rustc-link-lib=icui18n");
    }

    let is_shared = cfg!(windows) && std::env::var("VCPKGRS_DYNAMIC").is_ok();

    cc::Build::new()
        .file("wrapper/wrapper.cpp")
        .includes(includes)
        .include(dst.join("include"))
        .include(dst.join("include").join("cg3"))
        .include(dst)
        .static_flag(!is_shared)
        .static_crt(!is_shared)
        .cpp(true)
        .flag(if cfg!(windows) {
            "/std:c++14"
        } else {
            "-std=c++11"
        })
        .compile("cg3_wrapper");
}
