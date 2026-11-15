import CG3
import Foundation
import OSLog
import Testing

/// What a test's own calls logged. The settings are shared by every test, so
/// the handler keeps only the events raised on the thread `body` runs on: the
/// library sends each event on the thread that called it.
func capturing(at level: LogLevel?, _ body: () throws -> Void) rethrows -> [LogEvent] {
    let events = Captured()
    let thread = UInt(bitPattern: pthread_self())
    logLevel = level
    logHandler = { event in
        if UInt(bitPattern: pthread_self()) == thread { events.append(event) }
    }
    defer {
        logHandler = nil
        logLevel = .warning
    }
    try body()
    return events.all
}

final class Captured: @unchecked Sendable {
    private let lock = NSLock()
    private var events: [LogEvent] = []

    func append(_ event: LogEvent) {
        lock.withLock { events.append(event) }
    }

    var all: [LogEvent] {
        lock.withLock { events }
    }
}

/// Logging, whose settings every test shares, so these run one at a time.
@Suite(.serialized)
struct LoggingTests {
    /// Apertium input whose stray `$` the library reports, and carries on past.
    let strayDollar = "word$ ^word/word<n>$\n"
    let apertium = ApplyOptions(input: .apertium, output: .apertium)
    let select: Data

    init() throws {
        select = try Fixtures.data("contract/select.cg3b")
    }

    // [spec:cg3:req:swift.logging/test]
    @Test func theLevelIsWarningByDefault() {
        #expect(logLevel == .warning)
        #expect(logHandler == nil)
    }

    // [spec:cg3:req:swift.logging/test]
    @Test func whatARunLogsReachesTheHandlerAndApplyStillReturnsIt() throws {
        let grammar = try Grammar(cg3b: select)
        var warnings: [String] = []
        let events = try capturing(at: .warning) {
            warnings = try grammar.apply(to: strayDollar, options: apertium).warnings
        }
        #expect(warnings.count == 1)
        #expect(warnings.first?.contains("$ found without prior ^") == true)
        #expect(events.count == 1)
        #expect(events.first?.level == .error)
        #expect(events.first?.target == "cg3::apertium_applicator")
        #expect(events.first?.message == warnings.first)

        var quiet: [String] = []
        let none = try capturing(at: nil) {
            quiet = try grammar.apply(to: strayDollar, options: apertium).warnings
        }
        #expect(none.isEmpty)
        #expect(quiet == warnings)
    }

    // [spec:cg3:req:swift.logging/test]
    @Test func theLevelSetIsTheLeastLevelSent() throws {
        let grammar = try Grammar(cg3b: select)
        // With --verbose, JSONL logs a FLUSH at info, and an empty command at
        // warning.
        let commands = "{\"cmd\":\"<STREAMCMD:FLUSH>\"}\n{\"cmd\":\"\"}\n"
        let options = ApplyOptions(input: .jsonl, output: .jsonl, verbose: true)
        let info = try capturing(at: .info) { _ = try grammar.apply(to: commands, options: options) }
        let warning = try capturing(at: .warning) { _ = try grammar.apply(to: commands, options: options) }
        let none = try capturing(at: nil) { _ = try grammar.apply(to: commands, options: options) }
        #expect(info.contains { $0.level == .info && $0.message.contains("FLUSH encountered on line 1") })
        #expect(warning.map(\.level) == [.warning])
        #expect(warning.first?.message == "Warning: Empty 'cmd' value on line 2.")
        #expect(warning.first?.target == "cg3::jsonl_applicator")
        #expect(none.isEmpty)
    }

    // [spec:cg3:req:swift.logging/test]
    @Test func aHandlerMayCallIntoTheLibraryAgain() throws {
        let grammar = try Grammar(cg3b: select)
        let input = strayDollar
        let options = apertium
        let nested = Captured()
        logLevel = .warning
        logHandler = { event in
            if event.target == "cg3::apertium_applicator", let again = try? grammar.apply(to: "^a/a<n>$\n", options: options) {
                nested.append(LogEvent(level: .info, target: "nested", message: again.output))
            }
        }
        defer { logHandler = nil }
        _ = try grammar.apply(to: input, options: options)
        #expect(nested.all.contains { $0.message == "^a/a<n>$\n" })
    }

    // [spec:cg3:req:swift.logging/test]
    @Test func eventsReachTheUnifiedLogUnderTheSubsystem() throws {
        guard #available(macOS 12, iOS 15, *) else { return }
        let start = Date()
        let grammar = try Grammar(cg3b: select)
        _ = try capturing(at: .warning) { _ = try grammar.apply(to: strayDollar, options: apertium) }
        let store = try OSLogStore(scope: .currentProcessIdentifier)
        let entries = try store.getEntries(at: store.position(date: start))
            .compactMap { $0 as? OSLogEntryLog }
            .filter { $0.subsystem == "no.divvun.cg3" }
        let entry = try #require(entries.last)
        #expect(entry.category == "cg3::apertium_applicator")
        #expect(entry.level == .error)
    }
}
