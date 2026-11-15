---
id [dec:cg3:swift-package]
epitome "divvun.cg3 is one UniFFI static library, built with LTO and prelinked so that only its C interface is global, in an XCFramework for macOS, iOS and the iOS simulator, under two products over it: CG3, which runs compiled grammars as @divvun/cg3 does, and CG3Compile, which compiles them as @divvun/cg3/compile does. It is published as a source archive through pahkat's SwiftPM registry."
state @decided
category @existence
scope {
    elements ([arch:cg3:swift-package] [arch:cg3:embedding])
    rules (
        [spec:cg3:req:swift.package]
        [spec:cg3:req:swift.platforms]
        [spec:cg3:req:swift.one-library]
        [spec:cg3:req:swift.linking]
        [spec:cg3:req:swift.read-only-core]
        [spec:cg3:req:swift.surface]
        [spec:cg3:req:swift.grammar-load]
        [spec:cg3:req:swift.load-options]
        [spec:cg3:req:swift.apply]
        [spec:cg3:req:swift.tools]
        [spec:cg3:req:swift.errors]
        [spec:cg3:req:swift.concurrency]
        [spec:cg3:req:swift.logging]
        [spec:cg3:req:swift.external]
        [spec:cg3:req:swift.compile]
        [spec:cg3:req:swift.compile-includes]
        [spec:cg3:req:swift.compile-errors]
        [spec:cg3:req:swift.parity]
        [spec:cg3:req:swift.compile-parity]
    )
}
author "brendan@necessary.nu"
decided_at "2026-11-15T00:00:00Z"
edges {
    requires ([dec:cg3:wasm-package] [dec:cg3:tool-surface] [dec:cg3:package-versions])
}
alternatives (
    {
        option "Build with LTO alone, without the prelink."
        rejected_because "LTO internalises almost everything, but two std symbols stay global in every LTO-built Rust static library: `_rust_eh_personality` and `std::panicking::EMPTY_PANIC`. Linking cg3's library with an unrelated one built by the same toolchain failed with `ld: 2 duplicate symbols` naming them, whether the other was built with LTO or not."
    }
    {
        option "Build without LTO, as a plain static library."
        rejected_because "It defines 12,815 global symbols. It linked beside the unrelated library only by accident: the linker never loaded the second library's copies of std, uniffi_core and tracing, so both libraries ran on one copy, sharing statics such as tracing's global dispatcher. A different codegen-unit split in either build loads a member that redefines a symbol already loaded, and the link fails."
    }
    {
        option "A static library per product, one for CG3 and one for CG3Compile."
        rejected_because "Both would carry the library and std, so an app that uses both would carry them twice, and the two libraries would need to stay apart at link time like any two Rust libraries. One library under both products, with the linker dropping what an app does not call, keeps the compiler out of a CG3-only app all the same."
    }
    {
        option "Dynamic frameworks."
        rejected_because "Each is an image that loads at launch, which a keyboard extension pays for every time it starts, and each must be embedded and signed in every target that uses it. The linker cannot drop the compiler from inside a framework, so a CG3-only app would ship it."
    }
    {
        option "A C API written by hand and a Swift wrapper over it, as pahkat's mobile library has."
        rejected_because "Every type, option and error would be written twice by hand, once in C and once in Swift, and kept in step. UniFFI generates both sides from the Rust signatures, and hfst's Swift package uses it too."
    }
    {
        option "Make UniFFI's generated API the public API."
        rejected_because "It declares objects as open classes, labels every argument, cannot give a diagnostic's place as a `Range<String.Index>`, and throws a generated error type per crate. A thin hand-written layer gives Swift names, one `CG3Error` and final, `Sendable` types, and keeps the generated modules internal."
    }
    {
        option "Run the wasm build through a WebAssembly runtime in Swift."
        rejected_because "It would carry a runtime into every app and run the library slower than native. iOS forbids the JIT a fast WebAssembly runtime needs in an app extension, such as a keyboard."
    }
    {
        option "Publish from git, with a Package.swift at the repository root."
        rejected_because "A git dependency needs the XCFramework hosted elsewhere at a URL and checksum, or committed to the repository. A registry version carries it inside its signed source archive, and pahkat's registry serves Divvun's other Swift packages."
    }
    {
        option "Log through `os.Logger`."
        rejected_because "`os.Logger` needs iOS 14, and the package supports iOS 13. `os_log` with an `OSLog` for the subsystem and category reaches the same unified logging system on every version the package supports."
    }
)
consequences {
    accepted (
        "`crates/cg3-swift` is CG3's UniFFI crate and `crates/cg3-swift-compile` CG3Compile's. `crates/cg3-swift-ffi` is the static library that holds both, and its example runs uniffi-bindgen-swift at the version the crates link."
        "The static library is built with fat LTO and one codegen unit. Each slice is then prelinked into one object with `ld -r` and an exported symbols list naming only the C interface, as Xcode's single-object prelink does. The object keeps `MH_SUBSECTIONS_VIA_SYMBOLS`, so an app's linker still drops unused functions one by one. Measured on the iOS simulator, a dead-stripped binary that calls all of CG3's glue is 3.23 MB, and one that also calls CG3Compile's is 3.78 MB."
        "The XCFramework's headers lie in `Headers/CG3FFI`, whose module map declares the Clang modules `CG3BindingsFFI` and `CG3CompileBindingsFFI`, so no other package's module map lands on the same path."
        "The generated Swift is compiled into the internal modules `CG3Bindings` and `CG3CompileBindings`, which `CG3` and `CG3Compile` import with `internal import`. The manifest uses Swift tools 6.0, so an app needs Xcode 16 or later."
        "The iOS simulator slice is built for iOS 14, because Rust's `aarch64-apple-ios-sim` supports nothing older. The package and its device slice support iOS 13."
        "UniFFI catches a panic in the library, and the package throws it as `CG3Error` with code `panicked`."
        "`crates/cg3-swift` carries divvun.cg3's version, released from a `swift/v<version>` tag as `[dec:cg3:package-versions]` sets out for the other packages, starting at 0.1.0."
    )
    deferred (
        "Slices for Mac Catalyst, visionOS, tvOS and watchOS, and an x86_64 simulator slice for Intel Macs."
        "The binary stream format, over `Data`."
        "`async` forms of loading and applying."
    )
}
codifies (
    [spec:cg3:req:swift.linking]
    [spec:cg3:req:swift.read-only-core]
    [spec:cg3:req:swift.parity]
    [spec:cg3:req:swift.compile-parity]
)
establishes ([arch:cg3:swift-package])
---

## Rationale

The library's load-and-run API is what vislcg3 runs through, and what the
npm and Python packages bind. A Swift binding over it inherits the golden
corpus as its test, and the fixtures `js/test/fixtures.mjs` writes carry the
expected bytes to an iOS simulator, which cannot start vislcg3.

The main risk is linking. An iOS app such as giellakbd-ios links several
Rust static libraries, and each carries Rust's standard library. The linking
test on 2026-11-15 (rustc 1.99.0, Xcode 27.2, uniffi 0.32.2, the iOS
simulator) linked cg3's library with an unrelated Rust library built in its
own workspace with the same uniffi and tracing:

| cg3's library | the other | result |
|---|---|---|
| no LTO | no LTO | links, sharing one copy of std, uniffi_core and tracing |
| no LTO | LTO | 2 duplicate symbols |
| LTO | no LTO | 2 duplicate symbols |
| LTO | LTO | 2 duplicate symbols |
| LTO and prelinked | any of these, either order | links |

After the prelink cg3's library defines 144 global symbols, all of them its
C interface, and nothing another library defines can clash with it. The
build checks that, and the Swift linking test runs the three libraries
together on the simulator.

One library under two products gives the separation `@divvun/cg3` gets from
two `.wasm` files. Each product's generated Swift lives in a module of its
own, so an app that uses only `CG3` never references `CG3Compile`'s
functions, and the linker drops them and the compiler behind them.
