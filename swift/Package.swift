// swift-tools-version:6.0
// The package as it is developed and tested: Package.release.swift, the
// manifest each version's source archive carries, and the tests.
// `node swift/build.ts` builds CG3FFI.xcframework and the generated bindings
// these targets compile.
import PackageDescription

let package = Package(
    name: "cg3",
    platforms: [.iOS(.v13), .macOS(.v11)],
    products: [
        .library(name: "CG3", targets: ["CG3"]),
        .library(name: "CG3Compile", targets: ["CG3Compile"]),
    ],
    targets: [
        .binaryTarget(name: "CG3FFI", path: "CG3FFI.xcframework"),
        .target(name: "CG3Bindings", dependencies: ["CG3FFI"]),
        .target(name: "CG3", dependencies: ["CG3Bindings"]),
        .target(name: "CG3CompileBindings", dependencies: ["CG3FFI"]),
        .target(name: "CG3Compile", dependencies: ["CG3", "CG3CompileBindings"]),
        // The linking test's second Rust library, which build.ts builds
        // from link-probe on its own.
        .binaryTarget(name: "LinkProbeFFI", path: "LinkProbeFFI.xcframework"),
        .target(name: "LinkProbe", dependencies: ["LinkProbeFFI"], path: "Tests/LinkProbe"),
        .testTarget(name: "CG3Tests", dependencies: ["CG3", "CG3Compile", "LinkProbe"]),
    ]
)
