fn main() {
    let dst = cmake::build("cg3");

    println!("cargo:rustc-link-search=native={}", dst.display());
    println!("cargo:rustc-link-lib=static=cg3");
}
