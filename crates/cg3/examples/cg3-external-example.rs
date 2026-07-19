//! Reference implementation of a CG-3 `EXTERNAL` child process.
//!
//! The `vislcg3` engine's `EXTERNAL` directive spawns a helper program and
//! round-trips each window through it over a small binary protocol (host-order
//! `u32`/`u16`, length-prefixed; the handshake revision is `CG3_EXTERNAL_PROTOCOL`
//! in `grammar_applicator/run_rules/dispatch.rs`). This example is the other end
//! of that pipe: for every reading in every window it sets flag bit `1 << 0` and
//! appends the tag `"æ発ø"`, then writes the window back.
//!
//! It is the Rust port of the former `scripts/external.pl` +
//! `scripts/CG3_External.pm` fixture, and drives `test/T_External`
//! (`crates/cg3/tests/engine.rs::engine_external_pipe_protocol`).

use std::io::{self, BufReader, BufWriter, Read, Write};

/// Protocol revision the engine handshakes with (`CG3_EXTERNAL_PROTOCOL`).
const PROTOCOL: u32 = 7226;

/// Reading flag: a baseform string follows the flags word.
const R_FLAG_BASEFORM: u32 = 1 << 3;
/// Cohort flag: a text string follows the readings.
const C_FLAG_TEXT: u32 = 1 << 0;
/// Cohort flag: a parent index follows the flags word.
const C_FLAG_PARENT: u32 = 1 << 1;

struct Reading {
    flags: u32,
    baseform: Option<String>,
    tags: Vec<String>,
}

struct Cohort {
    num: u32,
    flags: u32,
    parent: Option<u32>,
    wordform: String,
    readings: Vec<Reading>,
    text: Option<String>,
}

struct Window {
    num: u32,
    cohorts: Vec<Cohort>,
}

// --- readers --------------------------------------------------------------

fn read_u32<R: Read>(r: &mut R) -> u32 {
    let mut b = [0u8; 4];
    r.read_exact(&mut b).expect("read u32");
    u32::from_ne_bytes(b)
}

/// Like `read_u32`, but a clean EOF at the read boundary yields `None` (used for
/// the leading window-packet-length word, which also ends the loop when zero).
fn try_read_u32<R: Read>(r: &mut R) -> Option<u32> {
    let mut b = [0u8; 4];
    match r.read_exact(&mut b) {
        Ok(()) => Some(u32::from_ne_bytes(b)),
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => None,
        Err(e) => panic!("read u32: {e}"),
    }
}

fn read_u16<R: Read>(r: &mut R) -> u16 {
    let mut b = [0u8; 2];
    r.read_exact(&mut b).expect("read u16");
    u16::from_ne_bytes(b)
}

/// A `u16` byte-length prefix followed by that many UTF-8 bytes.
fn read_string<R: Read>(r: &mut R) -> String {
    let n = read_u16(r) as usize;
    let mut buf = vec![0u8; n];
    r.read_exact(&mut buf).expect("read string bytes");
    String::from_utf8(buf).expect("utf8 string")
}

fn read_window<R: Read>(r: &mut R) -> Option<Window> {
    match try_read_u32(r) {
        None | Some(0) => return None, // EOF or explicit end-of-stream
        Some(_len) => {}               // window packet length (parsed structurally)
    }
    let num = read_u32(r);
    let clen = read_u32(r);
    let mut cohorts = Vec::with_capacity(clen as usize);
    for _ in 0..clen {
        let _cohort_packet_len = read_u32(r);
        let cnum = read_u32(r);
        let cflags = read_u32(r);
        let parent = (cflags & C_FLAG_PARENT != 0).then(|| read_u32(r));
        let wordform = read_string(r);
        let rlen = read_u32(r);
        let mut readings = Vec::with_capacity(rlen as usize);
        for _ in 0..rlen {
            let _reading_packet_len = read_u32(r);
            let rflags = read_u32(r);
            let baseform = (rflags & R_FLAG_BASEFORM != 0).then(|| read_string(r));
            let tlen = read_u32(r);
            let tags = (0..tlen).map(|_| read_string(r)).collect();
            readings.push(Reading {
                flags: rflags,
                baseform,
                tags,
            });
        }
        let text = (cflags & C_FLAG_TEXT != 0).then(|| read_string(r));
        cohorts.push(Cohort {
            num: cnum,
            flags: cflags,
            parent,
            wordform,
            readings,
            text,
        });
    }
    Some(Window { num, cohorts })
}

// --- writers --------------------------------------------------------------

fn w_u32(buf: &mut Vec<u8>, n: u32) {
    buf.extend_from_slice(&n.to_ne_bytes());
}

fn w_string(buf: &mut Vec<u8>, s: &str) {
    buf.extend_from_slice(&(s.len() as u16).to_ne_bytes());
    buf.extend_from_slice(s.as_bytes());
}

/// Perl truthiness for the optional baseform/text fields: absent, empty, and the
/// literal `"0"` are all false (`CG3_External.pm` gates on `if ($x)`).
fn present(s: &Option<String>) -> Option<&str> {
    s.as_deref().filter(|v| !v.is_empty() && *v != "0")
}

fn write_window<W: Write>(out: &mut W, w: &Window) {
    let mut wo = Vec::new();
    w_u32(&mut wo, w.num);
    w_u32(&mut wo, w.cohorts.len() as u32);
    for c in &w.cohorts {
        let mut co = Vec::new();
        w_u32(&mut co, c.num);
        let text = present(&c.text);
        let mut cflags = c.flags;
        if text.is_some() {
            cflags |= C_FLAG_TEXT;
        }
        if c.parent.is_some() {
            cflags |= C_FLAG_PARENT;
        }
        w_u32(&mut co, cflags);
        if let Some(p) = c.parent {
            w_u32(&mut co, p);
        }
        w_string(&mut co, &c.wordform);
        w_u32(&mut co, c.readings.len() as u32);
        for r in &c.readings {
            let mut ro = Vec::new();
            let baseform = present(&r.baseform);
            let mut rflags = r.flags;
            if baseform.is_some() {
                rflags |= R_FLAG_BASEFORM;
            }
            w_u32(&mut ro, rflags);
            if let Some(b) = baseform {
                w_string(&mut ro, b);
            }
            w_u32(&mut ro, r.tags.len() as u32);
            for t in &r.tags {
                w_string(&mut ro, t);
            }
            w_u32(&mut co, ro.len() as u32); // reading packet length
            co.extend_from_slice(&ro);
        }
        if let Some(t) = text {
            w_string(&mut co, t);
        }
        w_u32(&mut wo, co.len() as u32); // cohort packet length
        wo.extend_from_slice(&co);
    }
    out.write_all(&(wo.len() as u32).to_ne_bytes())
        .expect("write window length");
    out.write_all(&wo).expect("write window");
}

fn main() {
    let stdin = io::stdin();
    let mut r = BufReader::new(stdin.lock());
    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());

    let proto = read_u32(&mut r);
    if proto != PROTOCOL {
        eprintln!("Out of date protocol! got {proto}, expected {PROTOCOL}");
        std::process::exit(1);
    }

    while let Some(mut w) = read_window(&mut r) {
        for c in &mut w.cohorts {
            for reading in &mut c.readings {
                reading.flags |= 1 << 0;
                reading.tags.push("æ発ø".to_string());
            }
        }
        write_window(&mut out, &w);
        out.flush().expect("flush window");
    }
}
