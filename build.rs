use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=wrapper/wrapper.cpp");
    println!("cargo:rerun-if-changed=wrapper/wrapper.hpp");

    let mut dst = cmake::Config::new("cg3");

    let cg3_sysroot = std::env::var("CG3_SYSROOT").ok();

    let mut includes = if cfg!(windows) && cg3_sysroot.is_none() {
        let lib = vcpkg::Config::new().find_package("icu").unwrap();
        lib.include_paths
    } else {
        vec![]
    };
    if let Some(sysroot) = cg3_sysroot.as_ref() {
        let cg3_sysroot_path = PathBuf::from(sysroot);
        let pkgconfig_path = cg3_sysroot_path.join("lib").join("pkgconfig");
        dst.env("PKG_CONFIG_PATH", &pkgconfig_path);
        dst.define("CMAKE_PREFIX_PATH", &cg3_sysroot_path);

        // Bypass FindICU by setting ICU variables directly
        let lib_dir = cg3_sysroot_path.join("lib");
        dst.define("ICU_INCLUDE_DIRS", cg3_sysroot_path.join("include"));
        let (icu_uc, icu_i18n, icu_io, icu_data) = if cfg!(windows) {
            ("icuuc.lib", "icuin.lib", "icuio.lib", "icudt.lib")
        } else {
            ("libicuuc.a", "libicui18n.a", "libicuio.a", "libicudata.a")
        };
        dst.define(
            "ICU_LIBRARIES",
            format!(
                "{};{};{};{}",
                lib_dir.join(icu_uc).display(),
                lib_dir.join(icu_i18n).display(),
                lib_dir.join(icu_io).display(),
                lib_dir.join(icu_data).display()
            ),
        );

        includes.push(PathBuf::from(sysroot).join("include"));
    }

    #[cfg(windows)]
    let dst = {
        dst.static_crt(true);
        dst.define("WIN32", "ON")
            .define("MSVC", "ON")
            .define(
                "CMAKE_CXX_FLAGS",
                "/Dcg3_EXPORTS /DWIN32 /D_WIN32 /D_WINDOWS /W3 /GR /EHsc /O2 /MT",
            )
            .define("CMAKE_MSVC_RUNTIME_LIBRARY", "MultiThreaded")
            .define("BUILD_SHARED_LIBS", "OFF")
            .build()
    };

    #[cfg(unix)]
    let dst = {
        let dst = dst.define("BUILD_SHARED_LIBS", "OFF");
        dst.define("CMAKE_POSITION_INDEPENDENT_CODE", "ON");
        dst.define("CMAKE_C_COMPILER", "clang");
        dst.define("CMAKE_CXX_COMPILER", "clang++");

        let mut cflags: Vec<String> = includes
            .iter()
            .map(|x| format!("-I{}", x.display()))
            .collect();
        cflags.push("-fPIC".to_string());
        cflags.push("-flto=thin".to_string());

        dst.define("CMAKE_CXX_FLAGS", cflags.join(" "));
        dst.define("CMAKE_C_FLAGS", cflags.join(" "));

        dst.build()
    };

    let is_shared = cfg!(windows) && std::env::var("VCPKGRS_DYNAMIC").is_ok();

    let mut build = cc::Build::new();
    #[cfg(unix)]
    {
        build.compiler("clang++");
        build.flag("-fPIC");
        build.flag("-flto=thin");
    }
    build
        .file("wrapper/wrapper.cpp")
        .includes(includes)
        .include(dst.join("include"))
        .include(dst.join("include").join("cg3"))
        .include(&dst)
        .static_crt(!is_shared)
        .cpp(true)
        .flag(if cfg!(windows) {
            "/std:c++14"
        } else {
            "-std=c++20"
        });

    build.compile("cg3_wrapper");

    // Link directives must come AFTER cc::compile() to ensure correct link order:
    // cg3_wrapper (from cc) -> cg3 -> ICU
    println!("cargo:rustc-link-search=native={}/lib", dst.display());
    if let Some(sysroot) = cg3_sysroot.as_ref() {
        println!("cargo:rustc-link-search=native={}/lib", sysroot);
    }
    println!("cargo:rustc-link-lib=static:+whole-archive=cg3");

    if cfg!(unix) {
        println!("cargo:rustc-link-lib=static=icuuc");
        println!("cargo:rustc-link-lib=static=icuio");
        println!("cargo:rustc-link-lib=static=icudata");
        println!("cargo:rustc-link-lib=static=icui18n");
    } else if cfg!(windows) {
        println!("cargo:rustc-link-lib=icuuc");
        println!("cargo:rustc-link-lib=icuio");
        println!("cargo:rustc-link-lib=icudt");
        println!("cargo:rustc-link-lib=icuin");
    }
}
