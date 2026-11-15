//! The one static library divvun.cg3's products link, `libcg3_swift_ffi.a`:
//! the UniFFI glue of each, whose C interface the Swift package calls.
//! swift/build.ts builds it for every slice of the XCFramework.

// [spec:cg3:req:swift.one-library]
pub use cg3_swift;

#[cfg(test)]
mod tests {
    /// The library is versioned with the package, whose version the CG3 glue
    /// reports.
    #[test]
    fn the_library_has_the_package_version() {
        assert_eq!(env!("CARGO_PKG_VERSION"), cg3_swift::version());
    }
}
