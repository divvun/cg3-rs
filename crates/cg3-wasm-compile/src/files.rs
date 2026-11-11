//! The files a grammar's `INCLUDE`s read: those `compileGrammar` was given,
//! and none other, through the library's in-memory resolver.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use cg3::embed::{IncludeResolver, Included, MemoryResolver, UnreadableInclude};
use cg3_wasm_shared::{ErrorCode, GlueError};
use wasm_bindgen::{JsCast, JsValue};

// [spec:cg3:req:wasm.compile-includes]
/// The files given as name and content pairs, which the wrapper makes of a
/// record or a `Map`; none when `files` is `undefined`.
pub fn memory_files(files: &JsValue) -> Result<MemoryResolver, GlueError> {
    let mut resolver = MemoryResolver::default();
    if files.is_undefined() {
        return Ok(resolver);
    }
    let invalid = || {
        GlueError::new(
            ErrorCode::InvalidArgument,
            "the files are pairs of a name and a string or a Uint8Array",
        )
    };
    for pair in files.dyn_ref::<js_sys::Array>().ok_or_else(invalid)?.iter() {
        let pair = pair.dyn_into::<js_sys::Array>().map_err(|_| invalid())?;
        let name = pair.get(0).as_string().ok_or_else(invalid)?;
        let bytes = file_bytes(&pair.get(1)).ok_or_else(invalid)?;
        resolver.insert(&name, bytes);
    }
    Ok(resolver)
}

/// A file's content, given as text or bytes.
fn file_bytes(value: &JsValue) -> Option<Vec<u8>> {
    match value.as_string() {
        Some(text) => Some(text.into_bytes()),
        None => value
            .dyn_ref::<js_sys::Uint8Array>()
            .map(js_sys::Uint8Array::to_vec),
    }
}

// [spec:cg3:req:wasm.compile-includes]
/// The files a JavaScript function gives. Each `INCLUDE` reads through the
/// in-memory resolver; a name it does not hold is asked of the function where
/// the resolver looks for it, beside the including source and then among the
/// top-level names, as the resolver spells the name. What the function gives
/// is kept, so it is asked for each file once.
pub struct AskedFiles {
    function: js_sys::Function,
    files: RefCell<MemoryResolver>,
    thrown: Rc<RefCell<Option<JsValue>>>,
}

impl AskedFiles {
    pub fn new(function: js_sys::Function) -> AskedFiles {
        AskedFiles {
            function,
            files: RefCell::default(),
            thrown: Rc::default(),
        }
    }

    /// Where what the function threw, or a value from it that is no file, is
    /// left for the compile to throw once the parse it failed is over.
    pub fn thrown(&self) -> Rc<RefCell<Option<JsValue>>> {
        Rc::clone(&self.thrown)
    }

    /// The file the function gives for `name`, `None` for `undefined`.
    fn ask(&self, name: &str) -> Result<Option<Vec<u8>>, JsValue> {
        let value = self
            .function
            .call1(&JsValue::UNDEFINED, &JsValue::from_str(name))?;
        if value.is_undefined() {
            return Ok(None);
        }
        file_bytes(&value).map(Some).ok_or_else(|| {
            let message = format!(
                "the include function gave {} for {name}, not a string, a Uint8Array or undefined",
                value.js_typeof().as_string().unwrap_or_default()
            );
            GlueError::new(ErrorCode::InvalidArgument, message).into()
        })
    }
}

impl IncludeResolver for AskedFiles {
    fn read(&self, name: &str, includer: &str) -> Result<Included, UnreadableInclude> {
        let missing = match self.files.borrow().read(name, includer) {
            Ok(found) => return Ok(found),
            Err(missing) => missing,
        };
        let top_level = (!name.starts_with('/')).then_some(name);
        for looked in std::iter::once(missing.path.as_str()).chain(top_level) {
            let resolved = self.identity(looked).map_or_else(
                || looked.to_owned(),
                |path| path.to_string_lossy().into_owned(),
            );
            match self.ask(&resolved) {
                Ok(None) => {}
                Ok(Some(bytes)) => {
                    self.files.borrow_mut().insert(&resolved, bytes);
                    return self.files.borrow().read(name, includer);
                }
                Err(thrown) => {
                    *self.thrown.borrow_mut() = Some(thrown);
                    break;
                }
            }
        }
        Err(missing)
    }

    fn identity(&self, name: &str) -> Option<PathBuf> {
        self.files.borrow().identity(name)
    }
}
