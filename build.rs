use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=wrapper/wrapper.cpp");
    println!("cargo:rerun-if-changed=wrapper/wrapper.hpp");

    let mut dst = cmake::Config::new("cg3");

    let includes = if cfg!(windows) {
        let lib = vcpkg::Config::new().find_package("icu").unwrap();
        lib.include_paths
    } else if cfg!(target_os = "macos") {
        println!("cargo:rustc-link-search=native=/opt/homebrew/lib");
        vec![
            PathBuf::from("/opt/homebrew/include"),
        ]
    } else {
        vec![]
    };

    #[cfg(windows)]
    let dst = dst
        .define("WIN32", "ON")
        .define("MSVC", "ON")
        .define(
            "CMAKE_CXX_FLAGS",
            "/Dcg3_EXPORTS /DWIN32 /D_WIN32 /D_WINDOWS /W3 /GR /EHsc /O2",
        )
        .define("BUILD_SHARED_LIBS", "OFF")
        .build();

    #[cfg(unix)]
    let dst = {
        let dst = dst.define("BUILD_SHARED_LIBS", "OFF");
        dst.define("CMAKE_POSITION_INDEPENDENT_CODE", "ON");

        for x in includes.iter() {
            dst.define("CMAKE_CXX_FLAGS", format!("-I{}", x.display()));
            dst.define("CMAKE_C_FLAGS", format!("-I{}", x.display()));
        }

        dst.build()
    };

    println!("cargo:rustc-link-search=native={}/lib", dst.display());
    println!("cargo:rustc-link-lib=cg3");
    println!("cargo:rustc-link-lib=sqlite3");

    let is_shared = cfg!(windows) && std::env::var("VCPKGRS_DYNAMIC").is_ok();

    let mut build = cc::Build::new();
    build
        .file("wrapper/wrapper.cpp")
        .includes(includes)
        .include(dst.join("include"))
        .include(dst.join("include").join("cg3"))
        .include(&dst)
        .static_flag(!is_shared)
        .static_crt(!is_shared)
        .cpp(true)
        .flag(if cfg!(windows) {
            "/std:c++14"
        } else {
            "-std=c++20"
        });

    build.compile("cg3_wrapper");

    if cfg!(target_vendor = "apple") {
        println!("cargo:rustc-link-lib=icucore");
    } else {
        if cfg!(unix) {
            println!("cargo:rustc-link-lib=icuuc");
            println!("cargo:rustc-link-lib=icuio");
        } else if cfg!(windows) {
            println!("cargo:rustc-link-lib=icudt");
            println!("cargo:rustc-link-lib=icuin");
        }
        println!("cargo:rustc-link-lib=icudata");
        println!("cargo:rustc-link-lib=icui18n");
    }
}
