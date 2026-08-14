//! What an EMBEDDER sees on stderr — the path that does not go through
//! `run_cli`.
//!
//! The parser ports the C++ `throw int` parse-error recovery as
//! `panic_any(ParseError)`, caught once per directive so parsing can continue.
//! The default panic hook prints for every one of those, several frames before
//! the catch, so a library consumer got `thread 'main' panicked ... Box<dyn
//! Any>` for an ordinary bad grammar — reading like a crash next to the `Err`
//! it also returned.
//!
//! Asserting this needs a real process: panic output is a property of the
//! process's hook, not of a return value. The test re-executes its own binary
//! and reads the child's stderr.

use std::process::Command;

const CHILD: &str = "CG3_EMBED_DIAGNOSTICS_CHILD";

/// Two bad regex tags plus the rule that references the first set, so the
/// parser recovers more than once and would print more than one panic.
const BAD_GRAMMAR: &str = "DELIMITERS = \"<.>\" ;\n\
                           LIST a = \"[:script=Greek:]\"r ;\n\
                           LIST b = \"[:script=Latin:]\"r ;\n\
                           SELECT a ;\n";

fn child_loads_a_bad_grammar() {
    cg3::tools::init_diagnostics();
    let mut parser =
        cg3::textual_parser::TextualParser::new(cg3::grammar::Grammar::default(), false);
    let err = parser
        .parse_grammar_utf8(BAD_GRAMMAR.as_bytes())
        .expect_err("this grammar must not parse");
    // Print through the same channel an embedder would, so the parent can
    // confirm the child really reached the failure rather than exiting early.
    eprintln!("embedder saw: {err}");
}

// [spec:cg3:req:errors.control-flow-quiet/test]
#[test]
fn embedder_sees_no_panic_noise() {
    if std::env::var(CHILD).is_ok() {
        child_loads_a_bad_grammar();
        return;
    }

    let exe = std::env::current_exe().expect("path to this test binary");
    let out = Command::new(exe)
        .env(CHILD, "1")
        .args(["--exact", "embedder_sees_no_panic_noise", "--nocapture"])
        .output()
        .expect("re-exec the test binary");
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert!(out.status.success(), "child failed:\n{stderr}");
    // The child must actually have hit the recovery path — otherwise the
    // absence of panic noise below would prove nothing.
    assert!(
        stderr.contains("Error on line"),
        "child never reached the parse errors:\n{stderr}"
    );
    assert!(
        stderr.contains("embedder saw: grammar could not be parsed"),
        "child did not return the error to its caller:\n{stderr}"
    );

    assert!(
        !stderr.contains("panicked at"),
        "recoverable parse errors leaked Rust panic noise to an embedder:\n{stderr}"
    );
    assert!(
        !stderr.contains("Box<dyn Any>"),
        "control-flow payload reached stderr:\n{stderr}"
    );
}
