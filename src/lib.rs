use std::borrow::Cow;
use std::collections::VecDeque;
use std::sync::{Mutex, Once};
use std::{ffi::c_void, path::Path, str};

static START: Once = Once::new();

extern "C" {
    fn cg3_rs_init();
    fn cg3_applicator_new(
        grammar_data: *const u8,
        grammar_size: usize,
        grammar_ptr: *mut c_void,
    ) -> *mut c_void;
    fn cg3_applicator_delete(applicator: *mut c_void, grammar: *mut c_void);
    fn cg3_applicator_run(
        applicator: *mut c_void,
        input_data: *const u8,
        input_size: usize,
        output_size: *mut usize,
    ) -> *const u8;
    fn cg3_free(ptr: *const c_void);
    fn cg3_mwesplit_new() -> *mut c_void;
    fn cg3_mwesplit_delete(mwesplit: *mut c_void);
    fn cg3_mwesplit_run(
        mwesplit: *mut c_void,
        input_data: *const u8,
        input_size: usize,
        output_size: *mut usize,
    ) -> *const u8;
}

pub struct Applicator {
    applicator: *mut c_void,
    grammar: *mut c_void,
}

static CG3: Mutex<()> = Mutex::new(());

impl Applicator {
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        let _guard = CG3.lock().unwrap();

        let buf = std::fs::read(path).unwrap();

        START.call_once(|| unsafe { cg3_rs_init() });

        let grammar = std::ptr::null_mut();

        let applicator = unsafe { cg3_applicator_new(buf.as_ptr(), buf.len(), grammar) };
        Self {
            applicator,
            grammar,
        }
    }

    pub fn run(&self, input: &str) -> Option<String> {
        let mut output_size = 0usize;
        let output = unsafe {
            cg3_applicator_run(
                self.applicator,
                input.as_ptr(),
                input.len(),
                &mut output_size,
            )
        };
        let slice = unsafe { std::slice::from_raw_parts(output, output_size) };
        let out = std::str::from_utf8(slice).ok().map(|s| s.to_string());
        unsafe { cg3_free(output as _) };
        out
    }
}

impl Drop for Applicator {
    fn drop(&mut self) {
        unsafe { cg3_applicator_delete(self.applicator, self.grammar) };
    }
}

pub struct MweSplit {
    mwesplit: *mut c_void,
}

impl MweSplit {
    pub fn new() -> Self {
        let _guard = CG3.lock().unwrap();

        START.call_once(|| unsafe { cg3_rs_init() });
        let mwesplit = unsafe { cg3_mwesplit_new() };
        Self { mwesplit }
    }

    pub fn run(&self, input: &str) -> Option<String> {
        let mut output_size = 0usize;
        let output = unsafe {
            cg3_mwesplit_run(self.mwesplit, input.as_ptr(), input.len(), &mut output_size)
        };
        let slice = unsafe { std::slice::from_raw_parts(output, output_size) };
        let out = std::str::from_utf8(slice).ok().map(|s| s.to_string());
        unsafe { cg3_free(output as _) };
        out
    }
}

impl Drop for MweSplit {
    fn drop(&mut self) {
        unsafe { cg3_mwesplit_delete(self.mwesplit) };
    }
}

#[derive(Debug, Clone)]
pub struct Output<'a> {
    buf: Cow<'a, str>,
}

#[derive(Debug, Clone)]
pub enum Line<'a> {
    WordForm(&'a str),
    Reading(&'a str),
    Text(&'a str),
}

#[derive(Debug, Clone)]
pub enum Block<'a> {
    Cohort(Cohort<'a>),
    Escaped(&'a str),
    Text(&'a str),
}

#[derive(Debug, Clone)]
pub struct Reading<'a> {
    pub base_form: &'a str,
    pub tags: Vec<&'a str>,
    pub depth: usize,
}

#[derive(Debug, Clone)]
pub struct Cohort<'a> {
    pub word_form: &'a str,
    pub readings: Vec<Reading<'a>>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Invalid input: line {line}, position {position}, expected {expected}")]
    InvalidInput {
        line: usize,
        position: usize,
        expected: &'static str,
    },
}

impl<'a> Output<'a> {
    pub fn new<S: Into<Cow<'a, str>>>(buf: S) -> Self {
        let buf = buf.into();
        Self { buf }
    }

    fn lines(&'a self) -> impl Iterator<Item = Line<'a>> {
        let mut lines = self.buf.lines();
        std::iter::from_fn(move || {
            while let Some(line) = lines.next() {
                return Some(if line.starts_with("\"") {
                    Line::WordForm(line)
                } else if line.starts_with("\t") {
                    Line::Reading(line)
                } else {
                    Line::Text(line)
                });
            }
            None
        })
    }

    pub fn iter(&'a self) -> impl Iterator<Item = Result<Block<'a>, Error>> {
        let mut lines = self.lines().peekable();
        let mut cohort = None;
        let mut text = VecDeque::new();

        std::iter::from_fn(move || loop {
            if cohort.is_none() {
                if let Some(t) = text.pop_front() {
                    return Some(Ok(t));
                }
            }

            let Some(line) = lines.peek() else {
                if let Some(cohort) = cohort.take() {
                    return Some(Ok(Block::Cohort(cohort)));
                }

                return None;
            };

            let ret = loop {
                match line {
                    Line::WordForm(x) => {
                        if let Some(cohort) = cohort.take() {
                            return Some(Ok(Block::Cohort(cohort)));
                        }

                        let (Some(start), Some(end)) = (x.find("\"<"), x.find(">\"")) else {
                            return Some(Err(todo!()));
                        };

                        let word_form = &x[start + 2..end];

                        cohort = Some(Cohort {
                            word_form,
                            readings: Vec::new(),
                        });

                        break None;
                    }
                    Line::Reading(x) => {
                        let Some(cohort) = cohort.as_mut() else {
                            break Some(Err(todo!()));
                        };

                        let Some(depth) = x.rfind('\t') else {
                            break Some(Err(todo!()));
                        };

                        let x = &x[depth + 1..];
                        let mut chunks = x.split_ascii_whitespace();

                        let base_form = match chunks.next().ok_or_else(|| todo!()) {
                            Ok(v) => v,
                            Err(e) => break Some(Err(e)),
                        };

                        if !(base_form.starts_with("\"") && base_form.ends_with("\"")) {
                            todo!()
                        }
                        let base_form = &base_form[1..base_form.len() - 1];

                        cohort.readings.push(Reading {
                            base_form,
                            tags: chunks.collect(),
                            depth,
                        });

                        break None;
                    }
                    Line::Text(x) => {
                        if x.starts_with(':') {
                            text.push_back(Block::Escaped(&x[1..]));
                        } else {
                            text.push_back(Block::Text(x));
                        }

                        break None;
                    }
                }
            };

            lines.next();

            if let Some(ret) = ret {
                return Some(ret);
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse() {
        let output = Output::new(TEST_TEXT);
        for o in output.iter() {
            let o = o.unwrap();
            println!("{:?}", o);
        }
        let out = output
            .iter()
            .filter_map(Result::ok)
            .filter_map(|x| match x {
                Block::Cohort(x) => Some(x),
                _ => None,
            })
            .map(|x| x.word_form)
            .collect::<Vec<_>>();
        println!("{:?}", out);
    }

    const TEST_TEXT: &str = "\"<Wikipedia>\"
\t\"Wikipedia\" Err/Orth N Prop Sem/Org Attr <W:0.0>
\t\"Wikipedia\" Err/Orth N Prop Sem/Org Sg Acc <W:0.0>
\t\"Wikipedia\" Err/Orth N Prop Sem/Org Sg Gen <W:0.0>
\t\"Wikipedia\" Err/Orth N Prop Sem/Org Sg Nom <W:0.0>
\t\"Wikipedia\" N Prop Sem/Org Attr <W:0.0>
\t\"Wikipedia\" N Prop Sem/Org Sg Acc <W:0.0>
\t\"Wikipedia\" N Prop Sem/Org Sg Gen <W:0.0>
\t\"Wikipedia\" N Prop Sem/Org Sg Nom <W:0.0>
: 
\"<lea>\"
\t\"leat\" V IV Ind Prs Sg3 <W:0.0>
: 
\"<friddja>\"
\t\"friddja\" A Sem/Hum Attr <W:0.0>
\t\"friddja\" A Sem/Hum Sg Acc <W:0.0>
\t\"friddja\" A Sem/Hum Sg Gen <W:0.0>
\t\"friddja\" A Sem/Hum Sg Nom <W:0.0>
\t\"friddja\" Adv <W:0.0>
: 
\"<diehtosátnegirji>\"
\t\"sátnegirji\" N Sem/Txt Sg Nom <W:0.0>
\t\t\"diehtu\" N Sem/Prod-cogn_Txt Cmp/SgNom Cmp/SoftHyph Err/Orth Cmp <W:0.0>
\t\"girji\" N Sem/Txt Sg Nom <W:0.0>
\t\t\"sátni\" N Sem/Cat Cmp/SgNom Cmp <W:0.0>
\t\t\t\"diehtu\" N Sem/Prod-cogn_Txt Cmp/SgNom Cmp/SoftHyph Err/Orth Cmp <W:0.0>
\t\"sátnegirji\" N Sem/Txt Sg Nom <W:0.0>
\t\t\"dihto\" A Err/Orth Sem/Dummytag Cmp/Attr Cmp/SoftHyph Err/Orth Cmp <W:0.0>
\t\"girji\" N Sem/Txt Sg Nom <W:0.0>
\t\t\"sátni\" N Sem/Cat Cmp/SgNom Cmp <W:0.0>
\t\t\t\"dihto\" A Err/Orth Sem/Dummytag Cmp/Attr Cmp/SoftHyph Err/Orth Cmp <W:0.0>
: 
\"<badjel>\"
\t\"badjel\" Adv Sem/Plc <W:0.0>
\t\"badjel\" Adv Sem/Plc Gen <W:0.0>
\t\"badjel\" Po <W:0.0>
\t\"badjel\" Pr <W:0.0>
: 
\"<300>\"
\t\"300\" Num Arab Sg Acc <W:0.0>
\t\"300\" Num Arab Sg Gen <W:0.0>
\t\"300\" Num Arab Sg Ill Attr <W:0.0>
\t\"300\" Num Arab Sg Loc Attr <W:0.0>
\t\"300\" Num Arab Sg Nom <W:0.0>
\t\"300\" Num Sem/ID <W:0.0>
: 
\"<gielainn>\"
\t\"gielainn\" ?
\"<.>\"
\t\".\" CLB <W:0.0>
: 
";
}
