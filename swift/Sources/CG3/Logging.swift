import Foundation
import os.log
internal import CG3Bindings

/// The severity of a log event, from the least severe to the most.
public enum LogLevel: Int, Sendable, Hashable, Comparable, CaseIterable {
    /// What the library does step by step.
    case trace
    /// What helps find out why the library did what it did.
    case debug
    /// What the library reports on its progress, such as `--verbose` asks for.
    case info
    /// Something the library carries on past, such as an empty stream command.
    case warning
    /// Something wrong that the library still carries on past, such as a
    /// malformed reading; a failure that stops a call is thrown instead.
    case error

    /// Whether `lhs` is less severe than `rhs`.
    public static func < (lhs: LogLevel, rhs: LogLevel) -> Bool {
        lhs.rawValue < rhs.rawValue
    }
}

/// One event the library logged: loading a grammar, running one, or
/// compiling one.
public struct LogEvent: Sendable, Hashable {
    /// How severe it is.
    public let level: LogLevel
    /// Where in the library it came from, such as
    /// `cg3::grammar_applicator`.
    public let target: String
    /// What it says, followed by its other fields as `name=value`.
    public let message: String

    /// The event with these fields, as a handler is given it.
    public init(level: LogLevel, target: String, message: String) {
        self.level = level
        self.target = target
        self.message = message
    }
}

// [spec:cg3:req:swift.logging]
// [spec:cg3:req:swift.concurrency]
/// The least severe level the library's events are logged at, `warning` by
/// default; `nil` logs nothing. It applies to every product of the package.
/// Whatever it is, `apply(to:options:)` still returns its run's warnings.
public var logLevel: LogLevel? {
    get { Logging.settings.withLock { $0.level } }
    set { Logging.setLevel(newValue) }
}

// [spec:cg3:req:swift.logging]
// [spec:cg3:req:swift.concurrency]
/// Called with every event at `logLevel` or above, as it happens, besides
/// the unified logging system, which every event reaches under the subsystem
/// `no.divvun.cg3`. `nil`, the default, calls nothing. It is called on the
/// thread that called into the library, and may call into it again.
public var logHandler: (@Sendable (LogEvent) -> Void)? {
    get { Logging.settings.withLock { $0.handler } }
    set {
        Logging.install()
        Logging.settings.withLock { $0.handler = newValue }
    }
}

/// The logging settings, and the sink the library sends its events to.
package enum Logging {
    /// The unified logging system's subsystem for the package.
    static let subsystem = "no.divvun.cg3"

    struct Settings {
        var level: LogLevel? = .warning
        var handler: (@Sendable (LogEvent) -> Void)?
    }

    static let settings = Locked(Settings())

    /// Every category's log, made once for the first event of its target.
    static let logs = Locked([String: OSLog]())

    private static let installed: Void = {
        CG3Bindings.setLogSink(sink: Sink())
        settings.withLock { CG3Bindings.setLogLevel(level: $0.level.map(bindings)) }
    }()

    // [spec:cg3:req:swift.logging]
    /// Gives the library its sink and the level, once, before the first call
    /// into it; every product calls this before it calls into the library.
    package static func install() {
        _ = installed
    }

    /// Sets the level here and in the library together, under the lock, so
    /// the two never disagree.
    static func setLevel(_ level: LogLevel?) {
        install()
        settings.withLock {
            $0.level = level
            CG3Bindings.setLogLevel(level: level.map(bindings))
        }
    }

    static func bindings(_ level: LogLevel) -> CG3Bindings.LogLevel {
        switch level {
        case .trace: .trace
        case .debug: .debug
        case .info: .info
        case .warning: .warn
        case .error: .error
        }
    }

    static func level(_ level: CG3Bindings.LogLevel) -> LogLevel {
        switch level {
        case .trace: .trace
        case .debug: .debug
        case .info: .info
        case .warn: .warning
        case .error: .error
        }
    }

    // [spec:cg3:req:swift.logging]
    /// The type the unified logging system logs a level at: `os_log` has no
    /// warning or trace, so a warning is an error, as `Logger.warning` logs
    /// it, and trace is debug.
    static func type(_ level: LogLevel) -> OSLogType {
        switch level {
        case .trace, .debug: .debug
        case .info: .info
        case .warning, .error: .error
        }
    }

    static func log(for target: String) -> OSLog {
        logs.withLock { logs in
            if let log = logs[target] { return log }
            let log = OSLog(subsystem: subsystem, category: target)
            logs[target] = log
            return log
        }
    }

    // [spec:cg3:req:swift.logging]
    /// Logs `event` to the unified logging system, its message private, and
    /// hands it to the handler. The handler is read under the lock and called
    /// outside it, so it may change the settings.
    static func send(_ event: LogEvent) {
        os_log("%{private}@", log: log(for: event.target), type: type(event.level), event.message)
        settings.withLock { $0.handler }?(event)
    }

    /// What the library sends its events to, on the thread that called it.
    final class Sink: CG3Bindings.LogSink {
        func log(level: CG3Bindings.LogLevel, target: String, message: String) {
            send(LogEvent(level: Logging.level(level), target: target, message: message))
        }
    }
}
