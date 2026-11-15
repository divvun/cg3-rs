import Foundation

/// The files the tests read: what `js/test/fixtures.mjs` writes, as
/// `node swift/build.ts --test` writes it to target/swift/fixtures, or the
/// directory `CG3_SWIFT_FIXTURES` names. An iOS simulator reads the host's
/// files, so the same path serves it; `xcodebuild test` passes the variable
/// on as `TEST_RUNNER_CG3_SWIFT_FIXTURES`.
enum Fixtures {
    static let root = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent()
        .deletingLastPathComponent()
        .deletingLastPathComponent()
        .deletingLastPathComponent()

    static let directory: URL = {
        if let given = ProcessInfo.processInfo.environment["CG3_SWIFT_FIXTURES"], !given.isEmpty {
            return URL(fileURLWithPath: given)
        }
        return root.appendingPathComponent("target/swift/fixtures")
    }()

    static func data(_ path: String) throws -> Data {
        try Data(contentsOf: directory.appendingPathComponent(path))
    }

    static func text(_ path: String) throws -> String {
        String(decoding: try data(path), as: UTF8.self)
    }
}
