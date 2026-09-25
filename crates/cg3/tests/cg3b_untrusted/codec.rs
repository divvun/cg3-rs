//! A codec for the `.cg3b` wire layout documented in `cg3::binary_grammar`:
//! decodes a blob into its records so a test can damage one field and encode
//! the result again.

use std::collections::BTreeMap;

pub const BINF_PREFIX: u32 = 1 << 1;
pub const BINF_TAGS: u32 = 1 << 3;
pub const BINF_REOPEN_MAP: u32 = 1 << 4;
pub const BINF_PREF_TARGETS: u32 = 1 << 5;
pub const BINF_ENCLS: u32 = 1 << 6;
pub const BINF_ANCHORS: u32 = 1 << 7;
pub const BINF_SETS: u32 = 1 << 8;
pub const BINF_DELIMS: u32 = 1 << 9;
pub const BINF_SOFT_DELIMS: u32 = 1 << 10;
pub const BINF_CONTEXTS: u32 = 1 << 11;
pub const BINF_RULES: u32 = 1 << 12;
pub const BINF_TEXT_DELIMS: u32 = 1 << 16;

#[derive(Clone, Copy)]
enum Kind {
    U32,
    U8,
    I8,
    F64,
    Bytes,
    List,
    Names,
    Tries,
    Pos,
    Flags,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Field {
    U32(u32),
    U64(u64),
    U8(u8),
    I8(i8),
    F64([u8; 12]),
    Bytes(Vec<u8>),
    List(Vec<u32>),
    Names(Vec<Vec<u8>>),
    Tries(Vec<Entry>, Vec<Entry>),
    Pos(u64),
    /// Bytes written as they are, for shapes too deep to build as values.
    Raw(Vec<u8>),
}

/// One trie entry: tag number, terminal flag, children.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry(pub u32, pub u8, pub Vec<Entry>);

use Kind::*;

const TAG: &[(u32, Kind)] = &[
    (0, U32),
    (1, U32),
    (2, U32),
    (3, U32),
    (4, U32),
    (5, U32),
    (6, U32),
    (7, U32),
    (12, F64),
    (8, Bytes),
    (9, Bytes),
    (10, List),
    (11, Names),
    (13, U32),
    (14, U32),
];
const SET: &[(u32, Kind)] = &[
    (0, U32),
    (1, U32),
    (2, U8),
    (3, Tries),
    (4, List),
    (5, List),
    (6, Bytes),
];
const CONTEXT: &[(u32, Kind)] = &[
    (0, U32),
    (1, Pos),
    (2, U32),
    (3, U32),
    (4, U32),
    (5, U32),
    (6, U32),
    (7, U32),
    (8, U32),
    (9, U32),
    (12, I8),
    (10, List),
    (11, U32),
];
const RULE_HEAD: &[(u32, Kind)] = &[
    (0, U32),
    (1, U32),
    (2, U32),
    (3, Flags),
    (4, Bytes),
    (5, U32),
    (6, U32),
    (7, U32),
    (8, U32),
    (9, U32),
    (10, U32),
    (11, U32),
    (12, U32),
    (13, U32),
    (14, U32),
];

/// A record: its field mask and the fields the mask advertises.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Rec {
    pub mask: u32,
    pub fields: BTreeMap<u32, Field>,
}

impl Rec {
    pub fn set(&mut self, bit: u32, f: Field) {
        self.mask |= 1 << bit;
        self.fields.insert(bit, f);
    }
    pub fn clear(&mut self, bit: u32) {
        self.mask &= !(1 << bit);
        self.fields.remove(&bit);
    }
    pub fn has(&self, bit: u32) -> bool {
        self.mask & (1 << bit) != 0
    }
    pub fn u32(&self, bit: u32) -> u32 {
        match self.fields.get(&bit) {
            Some(Field::U32(v)) => *v,
            _ => 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RuleRec {
    pub head: Rec,
    pub dep: u32,
    pub dep_tests: Vec<u32>,
    pub tests: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Cg3b {
    pub rev: u32,
    pub fields: u32,
    pub prefix: Vec<u8>,
    pub cmdargs: Vec<u8>,
    pub cmdargs_override: Vec<u8>,
    pub tags: Vec<Rec>,
    pub reopen: Vec<u32>,
    pub preferred: Vec<u32>,
    pub parens: Vec<(u32, u32)>,
    pub anchors: Vec<(u32, u32)>,
    pub sets: Vec<Rec>,
    pub delims: [Option<u32>; 3],
    pub contexts: Vec<Rec>,
    pub rules: Vec<RuleRec>,
}

struct Rd<'a>(&'a [u8]);

impl Rd<'_> {
    fn take(&mut self, n: usize) -> &[u8] {
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        a
    }
    fn u32(&mut self) -> u32 {
        u32::from_be_bytes(self.take(4).try_into().unwrap())
    }
    fn u8(&mut self) -> u8 {
        self.take(1)[0]
    }
    fn bytes(&mut self) -> Vec<u8> {
        let n = self.u32() as usize;
        self.take(n).to_vec()
    }
    fn list(&mut self) -> Vec<u32> {
        (0..self.u32()).map(|_| self.u32()).collect()
    }
    fn trie(&mut self, n: u32) -> Vec<Entry> {
        (0..n)
            .map(|_| {
                let (tag, terminal, kids) = (self.u32(), self.u8(), self.u32());
                Entry(tag, terminal, self.trie(kids))
            })
            .collect()
    }
    fn field(&mut self, kind: Kind, mask: u32) -> Field {
        match kind {
            U32 => Field::U32(self.u32()),
            U8 => Field::U8(self.u8()),
            I8 => Field::I8(self.u8() as i8),
            F64 => Field::F64(self.take(12).try_into().unwrap()),
            Bytes => Field::Bytes(self.bytes()),
            List => Field::List(self.list()),
            Names => Field::Names((0..self.u32()).map(|_| self.bytes()).collect()),
            Tries => {
                let n = self.u32();
                let a = self.trie(n);
                let n = self.u32();
                Field::Tries(a, self.trie(n))
            }
            Pos => {
                let lo = u64::from(self.u32());
                let hi = if lo & (1 << 31) != 0 {
                    u64::from(self.u32())
                } else {
                    0
                };
                Field::Pos(lo | hi << 32)
            }
            Flags if mask & (1 << 16) != 0 => {
                Field::U64(u64::from(self.u32()) << 32 | u64::from(self.u32()))
            }
            Flags => Field::U32(self.u32()),
        }
    }
    fn rec(&mut self, schema: &[(u32, Kind)]) -> Rec {
        let mask = self.u32();
        let mut rec = Rec {
            mask,
            ..Rec::default()
        };
        for &(bit, kind) in schema {
            if mask & (1 << bit) != 0 {
                rec.fields.insert(bit, self.field(kind, mask));
            }
        }
        rec
    }
    fn counted<T>(&mut self, present: bool, mut one: impl FnMut(&mut Self) -> T) -> Vec<T> {
        let n = if present { self.u32() } else { 0 };
        (0..n).map(|_| one(self)).collect()
    }
}

#[derive(Default)]
pub struct Wr(pub Vec<u8>);

impl Wr {
    pub fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_be_bytes());
    }
    fn bytes(&mut self, b: &[u8]) {
        self.u32(b.len() as u32);
        self.0.extend_from_slice(b);
    }
    fn list(&mut self, l: &[u32]) {
        self.u32(l.len() as u32);
        l.iter().for_each(|&v| self.u32(v));
    }
    fn trie(&mut self, t: &[Entry]) {
        for Entry(tag, terminal, kids) in t {
            self.u32(*tag);
            self.0.push(*terminal);
            self.u32(kids.len() as u32);
            self.trie(kids);
        }
    }
    fn field(&mut self, f: &Field) {
        match f {
            Field::U32(v) => self.u32(*v),
            Field::U64(v) => self.0.extend_from_slice(&v.to_be_bytes()),
            Field::U8(v) => self.0.push(*v),
            Field::I8(v) => self.0.push(*v as u8),
            Field::F64(b) => self.0.extend_from_slice(b),
            Field::Bytes(b) => self.bytes(b),
            Field::List(l) => self.list(l),
            Field::Names(n) => {
                self.u32(n.len() as u32);
                n.iter().for_each(|b| self.bytes(b));
            }
            Field::Tries(a, b) => {
                self.u32(a.len() as u32);
                self.trie(a);
                self.u32(b.len() as u32);
                self.trie(b);
            }
            Field::Pos(p) => {
                self.u32(*p as u32);
                if p & (1 << 31) != 0 {
                    self.u32((p >> 32) as u32);
                }
            }
            Field::Raw(b) => self.0.extend_from_slice(b),
        }
    }
    fn rec(&mut self, r: &Rec, schema: &[(u32, Kind)]) {
        self.u32(r.mask);
        for &(bit, _) in schema {
            if r.has(bit) {
                self.field(&r.fields[&bit]);
            }
        }
    }
    fn counted<T>(&mut self, present: bool, items: &[T], mut one: impl FnMut(&mut Self, &T)) {
        if present {
            self.u32(items.len() as u32);
        }
        items.iter().for_each(|i| one(self, i));
    }
}

impl Cg3b {
    pub fn decode(blob: &[u8]) -> Cg3b {
        let mut r = Rd(blob);
        assert_eq!(r.take(4), b"CG3B");
        let (rev, fields) = (r.u32(), r.u32());
        let has = |bit| fields & bit != 0;
        let prefix = if has(BINF_PREFIX) {
            r.bytes()
        } else {
            Vec::new()
        };
        let (cmdargs, cmdargs_override) = (r.bytes(), r.bytes());
        let tags = r.counted(has(BINF_TAGS), |r| r.rec(TAG));
        let reopen = r.counted(has(BINF_REOPEN_MAP), Rd::u32);
        let preferred = r.counted(has(BINF_PREF_TARGETS), Rd::u32);
        let parens = r.counted(has(BINF_ENCLS), |r| (r.u32(), r.u32()));
        let anchors = r.counted(has(BINF_ANCHORS), |r| (r.u32(), r.u32()));
        let sets = r.counted(has(BINF_SETS), |r| r.rec(SET));
        let delims =
            [BINF_DELIMS, BINF_SOFT_DELIMS, BINF_TEXT_DELIMS].map(|b| has(b).then(|| r.u32()));
        let contexts = r.counted(has(BINF_CONTEXTS), |r| r.rec(CONTEXT));
        let rules = r.counted(has(BINF_RULES), |r| {
            let mut head = r.rec(RULE_HEAD);
            let (dep, dep_tests, tests) = (r.u32(), r.list(), r.list());
            if head.has(15) {
                head.fields.insert(15, Field::List(r.list()));
            }
            RuleRec {
                head,
                dep,
                dep_tests,
                tests,
            }
        });
        assert!(r.0.is_empty(), "codec left {} bytes", r.0.len());
        Cg3b {
            rev,
            fields,
            prefix,
            cmdargs,
            cmdargs_override,
            tags,
            reopen,
            preferred,
            parens,
            anchors,
            sets,
            delims,
            contexts,
            rules,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut w = Wr::default();
        w.0.extend_from_slice(b"CG3B");
        w.u32(self.rev);
        w.u32(self.fields);
        let has = |bit| self.fields & bit != 0;
        if has(BINF_PREFIX) {
            w.bytes(&self.prefix);
        }
        w.bytes(&self.cmdargs);
        w.bytes(&self.cmdargs_override);
        w.counted(has(BINF_TAGS), &self.tags, |w, t| w.rec(t, TAG));
        w.counted(has(BINF_REOPEN_MAP), &self.reopen, |w, &v| w.u32(v));
        w.counted(has(BINF_PREF_TARGETS), &self.preferred, |w, &v| w.u32(v));
        let pair = |w: &mut Wr, &(a, b): &(u32, u32)| {
            w.u32(a);
            w.u32(b);
        };
        w.counted(has(BINF_ENCLS), &self.parens, pair);
        w.counted(has(BINF_ANCHORS), &self.anchors, pair);
        w.counted(has(BINF_SETS), &self.sets, |w, s| w.rec(s, SET));
        self.delims.iter().flatten().for_each(|&d| w.u32(d));
        w.counted(has(BINF_CONTEXTS), &self.contexts, |w, c| w.rec(c, CONTEXT));
        w.counted(has(BINF_RULES), &self.rules, |w, r| {
            w.rec(&r.head, RULE_HEAD);
            w.u32(r.dep);
            w.list(&r.dep_tests);
            w.list(&r.tests);
            if let Some(f) = r.head.fields.get(&15) {
                w.field(f);
            }
        });
        w.0
    }

    /// The index (== number) of the tag whose text is `text`.
    pub fn tag(&self, text: &str) -> usize {
        let text = Field::Bytes(text.as_bytes().to_vec());
        self.tags
            .iter()
            .position(|t| t.fields.get(&8) == Some(&text))
            .unwrap_or_else(|| panic!("no tag {text:?}"))
    }

    /// The first record of `records` holding field `bit`.
    pub fn with(records: &[Rec], bit: u32) -> usize {
        records
            .iter()
            .position(|r| r.has(bit))
            .expect("a record with the field")
    }

    pub fn rule_with(&self, bit: u32) -> usize {
        self.rules
            .iter()
            .position(|r| r.head.has(bit))
            .expect("a rule with the field")
    }

    /// A set index whose `sets` field lists member sets.
    pub fn composite_set(&self) -> usize {
        Cg3b::with(&self.sets, 5)
    }
}
