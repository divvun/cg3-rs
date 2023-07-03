fn main() {
    let dst = cmake::Config::new("cg3")
        .define("BUILD_SHARED_LIBS", "off")
        .build();

    println!("cargo:rustc-link-search=native={}/lib", dst.display());

    let icu = pkg_config::Config::new()
        .statik(true)
        .probe("icu-uc")
        .unwrap();
    for path in icu.link_paths {
        println!("cargo:rustc-link-search=native={}", path.display());
    }

    println!("cargo:rustc-link-lib=static=cg3");
    println!("cargo:rustc-link-lib=static=icuuc");
    println!("cargo:rustc-link-lib=static=icuio");
    println!("cargo:rustc-link-lib=static=icudata");
    println!("cargo:rustc-link-lib=static=icui18n");

    cc::Build::new()
        .file("wrapper/wrapper.cpp")
        .include(dst.join("include"))
        .include(dst.join("include").join("cg3"))
        .static_flag(true)
        .cpp(true)
        .flag("-std=c++11")
        .compile("cg3_wrapper");
}
