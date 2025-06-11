use std::borrow::Cow;
use std::collections::VecDeque;
use std::fmt::Debug;
use std::sync::{Mutex, Once};
use std::{ffi::c_void, path::Path, str};

static START: Once = Once::new();

unsafe extern "C" {
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
        let _guard = CG3.lock().unwrap();
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
        let _guard = CG3.lock().unwrap();
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
        let _guard = CG3.lock().unwrap();
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
        let _guard = CG3.lock().unwrap();
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

#[derive(Clone)]
pub struct Reading<'a> {
    pub raw_line: &'a str,
    pub base_form: &'a str,
    pub tags: Vec<&'a str>,
    pub depth: usize,
}

impl Debug for Reading<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let alt = f.alternate();

        let mut x = f.debug_struct("Reading");
        x.field("base_form", &self.base_form)
            .field("tags", &self.tags)
            .field("depth", &self.depth);

        if alt {
            x.field("raw_line", &self.raw_line).finish()
        } else {
            x.finish_non_exhaustive()
        }
    }
}

impl std::fmt::Display for Reading<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}\"{}\"{}",
            "\t".repeat(self.depth),
            self.base_form,
            self.tags.iter().fold(String::new(), |mut acc, tag| {
                acc.push_str(" ");
                acc.push_str(tag);
                acc
            })
        )
    }
}

#[derive(Debug, Clone)]
pub struct Cohort<'a> {
    pub word_form: &'a str,
    pub readings: Vec<Reading<'a>>,
}

impl std::fmt::Display for Cohort<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "\"<{}>\"", self.word_form)?;
        for reading in &self.readings {
            writeln!(f, "{}", reading)?;
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Invalid input: line {line}, position {position}, expected {expected}")]
    InvalidInput {
        line: usize,
        position: usize,
        expected: &'static str,
    },
    #[error("Invalid line: {0}")]
    InvalidLine(String),
    #[error("Invalid reading: {0}")]
    InvalidReading(String),
}

impl std::fmt::Display for Output<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for block in self.iter() {
            match block {
                Ok(block) => write!(f, "{}", block)?,
                Err(_) => return Err(std::fmt::Error),
            }
        }
        Ok(())
    }
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

    pub fn sentences(&'a self) -> impl Iterator<Item = Result<String, Error>> {
        let mut iter = self.iter();

        std::iter::from_fn(move || {
            let mut sentence = String::new();

            while let Some(block) = iter.next() {
                let block = match block {
                    Ok(v) => v,
                    Err(e) => return Some(Err(e)),
                };

                match block {
                    Block::Cohort(cohort) => {
                        sentence.push_str(
                            cohort
                                .readings
                                .first()
                                .map(|r| r.base_form)
                                .unwrap_or(cohort.word_form),
                        );
                        if cohort
                            .readings
                            .first()
                            // Rudimentary check for sentence end. We want to include '.', '?', '!', but not commas.
                            .map(|x| x.base_form != "," && x.tags.contains(&"CLB"))
                            .unwrap_or(false)
                        {
                            return Some(Ok(sentence.trim().to_string()));
                        }
                    }
                    Block::Escaped(text) => {
                        let text = text.replace("\\n", "\n");
                        sentence.push_str(&text);
                    }
                    Block::Text(_text) => {}
                }
            }

            if !sentence.is_empty() {
                return Some(Ok(sentence.trim().to_string()));
            }

            None
        })
    }

    pub fn iter(&'a self) -> impl Iterator<Item = Result<Block<'a>, Error>> {
        let mut lines = self.lines().peekable();
        let mut cohort = None;
        let mut text = VecDeque::new();

        std::iter::from_fn(move || {
            loop {
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
                                return Some(Err(Error::InvalidLine(x.to_string())));
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
                                break Some(Err(Error::InvalidReading(x.to_string())));
                            };

                            let Some(depth) = x.rfind('\t') else {
                                break Some(Err(Error::InvalidReading(x.to_string())));
                            };

                            let x = &x[depth + 1..];
                            let mut chunks = tokenize_tags(x).into_iter();

                            let base_form = match chunks
                                .next()
                                .ok_or_else(|| Error::InvalidReading(x.to_string()))
                            {
                                Ok(v) => v,
                                Err(e) => break Some(Err(e)),
                            };

                            if !(base_form.starts_with("\"") && base_form.ends_with("\"")) {
                                break Some(Err(Error::InvalidReading(x.to_string())));
                            }
                            let base_form = &base_form[1..base_form.len() - 1];

                            cohort.readings.push(Reading {
                                raw_line: x,
                                base_form,
                                tags: chunks.collect(),
                                depth: depth + 1,
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
            }
        })
    }
}

impl std::fmt::Display for Block<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Block::Cohort(cohort) => {
                write!(f, "{}", cohort)
            }
            Block::Escaped(text) => writeln!(f, ":{}", text),
            Block::Text(text) => writeln!(f, "{}", text),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum TokenizeState {
    None,
    Token,
    InString,
    EndOfString,
}

fn tokenize_tags(input: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut state = TokenizeState::None;
    let mut cur = 0;

    for (i, c) in input.char_indices() {
        if c == '"' {
            if matches!(state, TokenizeState::None) {
                state = TokenizeState::InString;
                cur = i;
            } else if matches!(state, TokenizeState::InString) {
                state = TokenizeState::EndOfString;
            }
            continue;
        }

        if matches!(state, TokenizeState::EndOfString) && c.is_whitespace() {
            state = TokenizeState::None;
            tokens.push(&input[cur..i]);
            cur = i + 1;
            continue;
        }

        if c.is_whitespace() {
            if matches!(state, TokenizeState::None) {
                cur = i + 1;
            }
            if matches!(state, TokenizeState::Token) {
                tokens.push(&input[cur..i]);
                cur = i + 1;
                state = TokenizeState::None;
            }
            continue;
        } else {
            if matches!(state, TokenizeState::None) {
                state = TokenizeState::Token;
                continue;
            }
        }
    }

    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse() {
        println!("{}", TEST_TEXT);
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

    #[test]
    fn test_parse_round_trip() {
        let output = Output::new(TEST_TEXT);
        for line in output.iter() {
            println!("{:?}", line);
        }

        let reconstructed = output.to_string();
        assert_eq!(TEST_TEXT.trim(), reconstructed.trim());
    }

    #[test]
    fn test_parse_round_trip_2() {
        let output = Output::new(TEST_BADJEL);
        for line in output.iter() {
            println!("{:?}", line);
        }

        let reconstructed = output.to_string();
        assert_eq!(TEST_BADJEL.trim(), reconstructed.trim());
    }

    #[test]
    fn test_sentences() {
        let output = Output::new(TEST_BADJEL);
        for sentence in output.sentences() {
            println!("{:?}", sentence);
        }
    }

    #[test]
    fn test_tokenize_respecting_quotes() {
        // Test case 1: NRK example
        let input = r#""NRK" N Prop Sem/Org ACR Sg Nom <W:0.0> @HNOUN #1->0 "ænn ærr koo "phon"#;
        let tokens = tokenize_tags(input);
        let expected = vec![
            r#""NRK""#,
            "N",
            "Prop",
            "Sem/Org",
            "ACR",
            "Sg",
            "Nom",
            "<W:0.0>",
            "@HNOUN",
            "#1->0",
            r#""ænn ærr koo "phon"#,
        ];
        assert_eq!(tokens, expected);

        // Test case 2: New York example
        let input = r#""New York" MWE OLang/UND N Prop Sem/Plc Sg Ill <W:0.0> @<ADVL"#;
        let tokens = tokenize_tags(input);
        let expected = vec![
            r#""New York""#,
            "MWE",
            "OLang/UND",
            "N",
            "Prop",
            "Sem/Plc",
            "Sg",
            "Ill",
            "<W:0.0>",
            "@<ADVL",
        ];
        assert_eq!(tokens, expected);

        // Test case 3: Simple case without quotes
        let input = "word N Sg Nom";
        let tokens = tokenize_tags(input);
        let expected = vec!["word", "N", "Sg", "Nom"];
        assert_eq!(tokens, expected);
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

const TEST_BADJEL: &str = r#""<sáddejuvvot>"
	"sáddet" VV TVV Der/PassL <mv> <mv> V <TH-Acc-Any><SO-Loc-Any><DE-Ill-Any> <TH-Acc-Any><DE-Ill-*Ani> IV Ind Prs Sg2 <W:0> @+FMAINV #1->1
: 
"<báhpirat>"
	"bábir" N Sem/Mat_Txt Pl Nom <W:0> @<SUBJ #2->2
: 
"<interneahta>"
	"interneahtta" N Sem/Plc-abstr Sg Gen <W:0> @>P #3->4
: 
"<badjel>"
	"badjel" Po <W:0> @<ADVL &lex-bokte-not-badjel #4->4
	"bokte" Po <W:0> @<ADVL &SUGGEST #4->4
"<.>"
	"." CLB <W:0> #5->5
:\n
"<sáddejuvvot>"
	"sáddet" VV TVV Der/PassL <mv> <mv> V <TH-Acc-Any><SO-Loc-Any><DE-Ill-Any> <TH-Acc-Any><DE-Ill-*Ani> IV Ind Prs Sg2 <W:0> @+FMAINV #1->1
: 
"<báhpirat>"
	"bábir" N Sem/Mat_Txt Pl Nom <W:0> @<SUBJ #2->2
: 
"<interneahta>"
	"interneahtta" N Sem/Plc-abstr Sg Gen <W:0> @>P #3->4
: 
"<badjel>"
	"badjel" Po <W:0> @<ADVL &lex-bokte-not-badjel #4->4
	"bokte" Po <W:0> @<ADVL &SUGGEST #4->4
"<.>"
	"." CLB <W:0> #5->5
:
"#;
