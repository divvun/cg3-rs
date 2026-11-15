//! The files a grammar's `INCLUDE`s read: those `compileGrammar` was given,
//! from a dictionary or a closure, and none other, through the library's
//! in-memory resolver.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use cg3::embed::{IncludeResolver, Included, MemoryResolver, UnreadableInclude};

// [spec:cg3:req:swift.compile-includes]
/// The closure Swift was given, which takes a resolved name and returns the
/// file's text, or nothing for a file it does not have.
#[uniffi::export(with_foreign)]
pub trait IncludeFiles: Send + Sync {
    fn read(&self, name: String) -> Option<String>;
}

// [spec:cg3:req:swift.compile-includes]
/// The caller's files, as the library's in-memory resolver finds them, and
/// which of them each `INCLUDE` read, so that a diagnostic in an included file
/// can name the string the caller gave for it.
///
/// A name the resolver does not hold is asked of the closure, when there is
/// one, where the resolver looks for it: beside the including source, then
/// among the top-level names, as the resolver spells the name. What the
/// closure gives is kept, so it is asked for each file once.
pub struct CallerFiles {
    files: RefCell<MemoryResolver>,
    /// The name the caller gave each file under, by the file's identity.
    given: RefCell<HashMap<PathBuf, String>>,
    asked: Option<Arc<dyn IncludeFiles>>,
    /// The caller's name of each file read, by the name the parse calls it.
    read: Rc<RefCell<HashMap<String, String>>>,
}

impl CallerFiles {
    /// The files in `files`, by the names it gives them.
    pub fn given(files: HashMap<String, String>) -> CallerFiles {
        let caller = CallerFiles::new(None);
        for (name, text) in files {
            caller.keep(name, text);
        }
        caller
    }

    /// The files `asked` gives, when there is one. It is asked nothing until
    /// an `INCLUDE` names a file the resolver does not yet hold.
    pub fn new(asked: Option<Arc<dyn IncludeFiles>>) -> CallerFiles {
        CallerFiles {
            files: RefCell::default(),
            given: RefCell::default(),
            asked,
            read: Rc::default(),
        }
    }

    fn keep(&self, name: String, text: String) {
        self.files.borrow_mut().insert(&name, text);
        if let Some(identity) = self.files.borrow().identity(&name) {
            self.given.borrow_mut().insert(identity, name);
        }
    }

    /// Which file the caller gave each source the parse read from them, by
    /// the name the parse calls the source; it lives on after the parse.
    pub fn names_read(&self) -> Rc<RefCell<HashMap<String, String>>> {
        Rc::clone(&self.read)
    }

    fn found(&self, name: &str, includer: &str) -> Result<Included, UnreadableInclude> {
        let found = self.files.borrow().read(name, includer)?;
        let identity = self.files.borrow().identity(&found.name);
        if let Some(caller) = identity.and_then(|id| self.given.borrow().get(&id).cloned()) {
            self.read.borrow_mut().insert(found.name.clone(), caller);
        }
        Ok(found)
    }
}

impl IncludeResolver for CallerFiles {
    fn read(&self, name: &str, includer: &str) -> Result<Included, UnreadableInclude> {
        let missing = match self.found(name, includer) {
            Ok(found) => return Ok(found),
            Err(missing) => missing,
        };
        let Some(asked) = &self.asked else {
            return Err(missing);
        };
        let top_level = (!name.starts_with('/')).then_some(name);
        for looked in std::iter::once(missing.path.as_str()).chain(top_level) {
            let resolved = self.identity(looked).map_or_else(
                || looked.to_owned(),
                |path| path.to_string_lossy().into_owned(),
            );
            if let Some(text) = asked.read(resolved.clone()) {
                self.keep(resolved, text);
                return self.found(name, includer);
            }
        }
        Err(missing)
    }

    fn identity(&self, name: &str) -> Option<PathBuf> {
        self.files.borrow().identity(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Asked(HashMap<String, String>, std::sync::Mutex<Vec<String>>);

    impl IncludeFiles for Asked {
        fn read(&self, name: String) -> Option<String> {
            self.1.lock().unwrap().push(name.clone());
            self.0.get(&name).cloned()
        }
    }

    // [spec:cg3:req:swift.compile-includes/test]
    #[test]
    fn files_resolve_as_on_disk() {
        let files = CallerFiles::given(HashMap::from([
            ("./lists/sets.cg3".to_owned(), "LIST N = n ;".to_owned()),
            ("rules.cg3".to_owned(), "SELECT N ;".to_owned()),
        ]));
        let sets = files.read("sets.cg3", "lists/main.cg3").unwrap();
        assert_eq!(sets.bytes, b"LIST N = n ;");
        let rules = files.read("rules.cg3", "lists/main.cg3").unwrap();
        assert_eq!(rules.name, "rules.cg3");
        let read = files.names_read();
        assert_eq!(read.borrow()[&sets.name], "./lists/sets.cg3");
        assert_eq!(read.borrow()[&rules.name], "rules.cg3");
        assert!(files.read("nowhere.cg3", "grammar.cg3").is_err());
    }

    // [spec:cg3:req:swift.compile-includes/test]
    #[test]
    fn the_closure_is_asked_once() {
        let asked = Arc::new(Asked(
            HashMap::from([("rules.cg3".to_owned(), "SELECT N ;".to_owned())]),
            Default::default(),
        ));
        let files = CallerFiles::new(Some(asked.clone()));
        let rules = files.read("rules.cg3", "lists/main.cg3").unwrap();
        assert_eq!(rules.bytes, b"SELECT N ;");
        assert_eq!(*asked.1.lock().unwrap(), ["lists/rules.cg3", "rules.cg3"]);
        files.read("./rules.cg3", "grammar.cg3").unwrap();
        assert_eq!(asked.1.lock().unwrap().len(), 2, "a file given is kept");
        assert_eq!(files.names_read().borrow()[&rules.name], "rules.cg3");
        assert!(files.read("gone.cg3", "grammar.cg3").is_err());
    }
}
