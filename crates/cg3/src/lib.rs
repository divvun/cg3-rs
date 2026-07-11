//! VISL CG-3 (Constraint Grammar) engine — Rust port.
//!
//! Wave 2 of the nplan port: a literal, bug-for-bug 1:1 translation of the C++
//! sources under `../../../src`. UTF-8 throughout (see [`types`]); idiomatic
//! cleanups are deferred to Wave 4. Every ported item carries its
//! `[spec:cg3:...]` annotation tying it back to the spec rule it implements.
//!
//! Snake_case C++ type names are preserved where practical; `non_camel_case_types`
//! is allowed crate-wide so the 1:1 mapping reads cleanly against the source.
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(dead_code)]

// --- Wave 2 foundation layer (pointer-agnostic: containers + utilities) ---
pub mod types;
pub mod error;
pub mod arena;
pub mod inlines;
pub mod sorted_vector;
pub mod interval_vector;
pub mod flat_unordered_map;
pub mod flat_unordered_set;
pub mod bloomish;
pub mod pool;
pub mod scoped_stack;
pub mod math_parser;
pub mod strings;

// --- Wave 2 core data model (type skeleton; method bodies land next) ---
pub mod tag;
pub mod tag_trie;
pub mod set;
pub mod rule;
pub mod contextual_test;
pub mod reading;
pub mod cohort;
pub mod cohort_iterator;
pub mod single_window;
pub mod window;
pub mod grammar;
pub mod store;

// --- Wave 2 support utilities (io / platform / parser-support / options) ---
pub mod uextras;
pub mod streambuf;
pub mod process;
pub mod filesystem;
pub mod ast;
pub mod igrammar_parser;
pub mod options;
pub mod options_parser;
pub mod options_conv;
pub mod icu_uoptions;

// --- Wave 2 parser + serialization layer ---
pub mod parser_helpers;
pub mod textual_parser;
pub mod binary_grammar;
pub mod grammar_writer;

// --- Wave 2 application engine ---
pub mod grammar_applicator;

// --- Wave 2 output/format applicators + profiler + relabeller ---
pub mod apertium_applicator;
pub mod matxin_applicator;
pub mod binary_applicator;
pub mod fst_applicator;
pub mod jsonl_applicator;
pub mod format_converter;
pub mod niceline_applicator;
pub mod plaintext_applicator;
pub mod mwesplit_applicator;
pub mod profiler;
pub mod relabeller;

// --- Wave 2 CLI tool entry points ---
pub mod tools;
