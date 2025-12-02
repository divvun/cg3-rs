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
        dst.define(
            "ICU_LIBRARIES",
            format!(
                "{};{};{};{}",
                lib_dir.join("libicuuc.a").display(),
                lib_dir.join("libicui18n.a").display(),
                lib_dir.join("libicuio.a").display(),
                lib_dir.join("libicudata.a").display()
            ),
        );

        includes.push(PathBuf::from(sysroot).join("include"));
    }
    
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

        let includes = includes
            .iter()
            .map(|x| format!("-I{}", x.display()))
            .collect::<Vec<_>>();

        dst.define("CMAKE_CXX_FLAGS", includes.join(" "));
        dst.define("CMAKE_C_FLAGS", includes.join(" "));

        dst.build()
    };

    let is_shared = cfg!(windows) && std::env::var("VCPKGRS_DYNAMIC").is_ok();

    let mut build = cc::Build::new();
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
    println!("cargo:rustc-link-lib=static=cg3");

    if cfg!(unix) {
        println!("cargo:rustc-link-lib=static=icuuc");
        println!("cargo:rustc-link-lib=static=icuio");
        println!("cargo:rustc-link-lib=static=icudata");
        println!("cargo:rustc-link-lib=static=icui18n");
    } else if cfg!(windows) {
        println!("cargo:rustc-link-lib=icudt");
        println!("cargo:rustc-link-lib=icuin");
        println!("cargo:rustc-link-lib=icudata");
        println!("cargo:rustc-link-lib=icui18n");
    }
}
