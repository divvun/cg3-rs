// swift-tools-version:6.0
// The package as it is developed and tested. The build writes the manifest
// each version's source archive carries from Package.release.swift; this one
// adds the tests. `node swift/build.ts` builds CG3FFI.xcframework and the
// generated bindings these targets compile.
import PackageDescription

let package = Package(
    name: "cg3",
    platforms: [.iOS(.v13), .macOS(.v11)],
    products: [
        .library(name: "CG3", targets: ["CG3"]),
    ],
    targets: [
        .binaryTarget(name: "CG3FFI", path: "CG3FFI.xcframework"),
        .target(name: "CG3Bindings", dependencies: ["CG3FFI"]),
        .target(name: "CG3", dependencies: ["CG3Bindings"]),
        .testTarget(name: "CG3Tests", dependencies: ["CG3"]),
    ]
)
