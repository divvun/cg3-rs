use std::path::PathBuf;

fn main() {
    // #[cfg(windows)]
    // let sysroot = PathBuf::from(std::env::var("SYSROOT").unwrap());

    #[cfg(windows)]
    let dst = cmake::Config::new("cg3")
        .define("WIN32", "ON")
        .define("MSVC", "ON")
        .define("CMAKE_CXX_FLAGS", "/Dcg3_EXPORTS /DWIN32 /D_WIN32 /D_WINDOWS /W3 /GR /EHsc")
        // .define("CMAKE_TOOLCHAIN_FILE", r"D:\vcpkg\scripts\buildsystems\vcpkg.cmake")
        // .define("SQLITE3_INCLUDE_DIRS", r"D:\vcpkg\installed\x64-windows\include")
        // .define("SQLITE3_LIBRARIES", r"D:\vcpkg\installed\x64-windows\lib\sqlite3.lib")
        // .define("ICU_LIBRARY_DIRS", sysroot.join("lib64"))
        .define("BUILD_SHARED_LIBS", "OFF")
        // .define("Boost_INCLUDE_DIR", sysroot.join("include"))
        // .define("ICU_INCLUDE_DIR", sysroot.join("include"))
        .build();

    #[cfg(unix)]
    let dst = cmake::Config::new("cg3")
        .define("BUILD_SHARED_LIBS", "OFF")
        .build();

    println!("cargo:rustc-link-search=native={}/lib", dst.display());

    if cfg!(unix) {
        let icu = pkg_config::Config::new()
            .statik(true)
            .probe("icu-uc")
            .unwrap();
        for path in icu.link_paths {
            // println!("cargo:rustc-link-search=native={}", path.display());
        }
    } else {
        // println!("cargo:rustc-link-search=native={}", r"D:\sysroot\lib64");
    }

    println!("cargo:rustc-link-lib=static=cg3");

    if cfg!(unix) {
        println!("cargo:rustc-link-lib=static=icuuc");
        println!("cargo:rustc-link-lib=static=icuio");
        if cfg!(windows) {
            println!("cargo:rustc-link-lib=static=icudt");
            println!("cargo:rustc-link-lib=static=icuin");
        } else {
            println!("cargo:rustc-link-lib=static=icudata");
            println!("cargo:rustc-link-lib=static=icui18n");
        }
    }

    cc::Build::new()
        .file("wrapper/wrapper.cpp")
        .include(dst.join("include"))
        .include(dst.join("include").join("cg3"))
        .include(dst)
        // .include(sysroot)
        .static_flag(true)
        .cpp(true)
        // .flag("-std=c++11")
        .compile("cg3_wrapper");
}
