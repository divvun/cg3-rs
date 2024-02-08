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
