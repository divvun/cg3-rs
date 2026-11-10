---
id [dec:cg3:package-versions]
epitome "@divvun/cg3 and divvun-cg3 are versioned on their own, each in its binding crate, and each is released from its own tag: v<version> the library and tools, wasm/v<version> the npm package, python/v<version> the Python package."
state @decided
category @existence
scope {
    elements ([arch:cg3:wasm-package] [arch:cg3:python-package])
    rules (
        [spec:cg3:req:wasm.module]
        [spec:cg3:req:python.module]
    )
}
author "brendan@necessary.nu"
decided_at "2026-11-10T00:00:00Z"
edges {
    requires ([dec:cg3:wasm-package] [dec:cg3:python-package])
}
alternatives (
    {
        option "Publish both packages at the library's version, from the library's `v<version>` tag."
        rejected_because "A fix to a binding alone would need a library release, and every library release would publish both packages again with nothing in them changed. A package's version would also say nothing about its own surface: a breaking change to the Python API could ride a patch release of the library."
    }
)
consequences {
    accepted (
        "`crates/cg3-wasm` and `crates/cg3-python` carry their package's version rather than the workspace's, and depend on the library at its own version. `version()` and `cg3.__version__` report the package's version."
        "CI builds what a tag releases: `v<version>` the tools and the GitHub release, `wasm/v<version>` the npm package, `python/v<version>` the wheels and the sdist. A build of `main` builds all of them."
        "divvun-cg3 starts at 0.1.0. @divvun/cg3 goes on from 0.5.0, the version npm already has."
    )
    deferred (
        "Reporting the library version a package was built from, beside the package's own."
    )
}
codifies ([spec:cg3:req:wasm.module] [spec:cg3:req:python.module])
---

## Rationale

The library, the npm package and the Python package change at different
rates. Most library releases fix the engine and leave both binding surfaces
alone, and most binding releases change a binding and leave the engine alone.
One version for all three made every release of one a release of the others.

Each binding crate is unpublished and exists only to be its package, so its
own version is the package's: maturin reads it for the wheels, the npm
package's manifest is checked against it, and the bindings report it at run
time. The library and the tools keep one version between them, as
`crate-split.one-version` requires.
