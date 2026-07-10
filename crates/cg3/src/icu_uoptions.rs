//! Port of `src/icu_uoptions.cpp` (`u_parseArgs`) — an ICU-derived getopt-style
//! command-line parser.
//!
//! ## DEAD CODE (NOTE / reconcile)
//! The C++ translation unit `src/icu_uoptions.cpp` is **not** in the CMake
//! build. It `#include`s a non-existent `icu_uoptions.hpp`, and it reads
//! `option->optionFn` / `option->context` — members that do **not** exist on
//! the live `UOption` struct — so it would not even compile against the real
//! header. The parser actually linked into every binary is the *identical*
//! inline `u_parseArgs` in the vendored `include/uoptions.hpp` (out of scope),
//! which is byte-for-byte the same algorithm MINUS the `optionFn` callback
//! block, and which uses `strcmp` / `size_t optionCount`.
//!
//! Per the spec sem note ("Port the live (header) behavior; the callback path
//! here is dead"), this port reproduces the shared algorithm operating on the
//! live [`crate::options::UOption`], and **omits** the dead `optionFn` callback
//! block (it cannot be expressed — `UOption` has no `optionFn`/`context`). The
//! one place it would have run is marked below.
//!
//! Faithful-to-`icu_uoptions.cpp` residue that survives: the signature keeps
//! `optionCount: i32` (the dead file's `int`, vs the header's `size_t`) and the
//! comment notes `uprv_strcmp` (an ICU macro) where the header uses `strcmp`.
//!
//! Because `options_parser::parse_opts` needs a `u_parseArgs` to call and the
//! live header is out of scope, `parse_opts` currently routes here (the logic
//! is identical to the live version). RECONCILE: once `include/uoptions.hpp` is
//! ported, repoint `parse_opts` at that and drop/retire this module.
//!
//! ## `char* argv[]` representation (NOTE)
//! The C `argv` is an array of NUL-terminated C strings that this function
//! reads char-by-char (`arg[0]`, `arg[1]`, `arg += 2`, …) and compacts in
//! place. It is modelled here as `&mut [Vec<char>]` where each token is a
//! NUL-free `Vec<char>` (the terminator is represented by "index past the end";
//! see [`at`]). `option->value = argv[i]` / `= arg` (which copy the C string
//! into the live `std::string value`) become `String` collects.

use crate::options::{UOPT_NO_ARG, UOPT_REQUIRES_ARG, UOption};
use crate::types::UChar;

/// Reads the `k`-th `char` of a NUL-free token, returning `'\0'` for any index
/// at or past the end. This is the `arg[k]` / `*arg`-style access from the C
/// original, where reading at/after the terminator yields the NUL byte.
#[inline]
fn at(token: &[UChar], k: usize) -> UChar {
    if k < token.len() { token[k] } else { '\0' }
}

// [spec:cg3:def:icu-uoptions.u-parse-args-fn]
// [spec:cg3:sem:icu-uoptions.u-parse-args-fn]
pub fn u_parseArgs(argc: i32, argv: &mut [Vec<UChar>], option_count: i32, options: &mut [UOption]) -> i32 {
    let mut i: i32 = 1;
    let mut remaining: i32 = 1;
    let mut stop_options = false;

    while i < argc {
        // arg = argv[i]
        let iu = i as usize;
        if !stop_options && at(&argv[iu], 0) == '-' && at(&argv[iu], 1) != '\0' {
            // process an option
            let mut c: UChar = at(&argv[iu], 1);
            // arg += 2 (past "-X"); tracked as an offset into argv[i]
            let mut arg_off: usize = 2;

            if c == '-' {
                // process a long option
                if at(&argv[iu], arg_off) == '\0' {
                    // stop processing options after "--"
                    stop_options = true;
                }
                else {
                    // search for the option string (uprv_strcmp in the dead
                    // file; strcmp in the live header) — exact match
                    let name: String = argv[iu][arg_off..].iter().collect();
                    let mut option: Option<usize> = None;
                    for j in 0..option_count as usize {
                        if let Some(ln) = options[j].long_name {
                            if name == ln {
                                option = Some(j);
                                break;
                            }
                        }
                    }
                    let opt = match option {
                        Some(j) => j,
                        // no option matches
                        None => return -i,
                    };
                    options[opt].does_occur = true;

                    if options[opt].has_arg != UOPT_NO_ARG {
                        // parse the argument for the option, if any
                        if i + 1 < argc
                            && !(at(&argv[(i + 1) as usize], 0) == '-'
                                && at(&argv[(i + 1) as usize], 1) != '\0')
                        {
                            // argument in the next argv[], and there is not an option in there
                            i += 1;
                            options[opt].value = argv[i as usize].iter().collect();
                        }
                        else if options[opt].has_arg == UOPT_REQUIRES_ARG {
                            // there is no argument, but one is required: return with error
                            return -i;
                        }
                    }
                }
            }
            else {
                // process one or more short options
                loop {
                    // search for the option letter
                    let mut option: Option<usize> = None;
                    for j in 0..option_count as usize {
                        if c == options[j].short_name {
                            option = Some(j);
                            break;
                        }
                    }
                    let opt = match option {
                        Some(j) => j,
                        // no option matches
                        None => return -i,
                    };
                    options[opt].does_occur = true;

                    if options[opt].has_arg != UOPT_NO_ARG {
                        // parse the argument for the option, if any
                        if at(&argv[iu], arg_off) != '\0' {
                            // argument following in the same argv[]
                            options[opt].value = argv[iu][arg_off..].iter().collect();
                            // do not process the rest of this arg as option letters
                            break;
                        }
                        else if i + 1 < argc
                            && !(at(&argv[(i + 1) as usize], 0) == '-'
                                && at(&argv[(i + 1) as usize], 1) != '\0')
                        {
                            // argument in the next argv[], and there is not an option in there
                            i += 1;
                            options[opt].value = argv[i as usize].iter().collect();
                            // this break is redundant because we know that *arg==0
                            break;
                        }
                        else if options[opt].has_arg == UOPT_REQUIRES_ARG {
                            // there is no argument, but one is required: return with error
                            return -i;
                        }
                    }

                    // get the next option letter: c = *arg++;
                    c = at(&argv[iu], arg_off);
                    arg_off += 1;
                    if c == '\0' {
                        break;
                    }
                }
            }

            // DEAD optionFn callback block (icu_uoptions.cpp only):
            //   if (option != 0 && option->optionFn != 0 &&
            //       option->optionFn(option->context, option) < 0) return -i;
            // Omitted: the live `UOption` has no `optionFn`/`context` members, so
            // this path never existed in any built binary (see module NOTE).

            // go to next argv[]
            i += 1;
        }
        else {
            // move a non-option up in argv[]: argv[remaining++] = arg;
            let a = argv[iu].clone();
            argv[remaining as usize] = a;
            remaining += 1;
            i += 1;
        }
    }
    remaining
}
