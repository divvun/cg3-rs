// swift-tools-version:6.0
// The manifest of divvun.cg3 as each version's source archive carries it,
// beside CG3FFI.xcframework; swift/build.ts --release puts it there as
// Package.swift.
// [spec:cg3:req:swift.package]
// [spec:cg3:req:swift.platforms]
// [spec:cg3:req:swift.one-library]
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
    ]
)
