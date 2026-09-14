# Vendored spytial-core assets

These files are vendored from [spytial-core](https://github.com/sidprasad/spytial-core)
at the version recorded in `VERSION.txt`. The four browser assets are bundled into the
rendered HTML by `src/lib.rs` at compile time via `include_str!`, so `dbg!`/`diagram`
works offline and without network access.

To update, run the script — don't copy files by hand:

```bash
scripts/update-spytial-core.sh 6.0.0
```

It pulls the published tarball (a local `npm run build:all` produces the same bytes,
but the tarball is what consumers actually get), copies the files below, rewrites
`VERSION.txt`, and regenerates `macros/src/spec_tables.rs` and
`macros/src/attributes.md` from the new manifest.

That last step is why the script exists rather than a checklist: the derive macro's
accepted keys and compile-time validation are *derived* from `spytial-language.json`,
so bumping the version without regenerating leaves the macro describing the previous
language. `spec-codegen`'s drift test catches it in CI, but the script is what keeps
the two in step to begin with.

| File in this directory | Source in spytial-core |
|---|---|
| `spytial-core.global.js` | `dist/browser/spytial-core-complete.global.js` |
| `spytial-core.css` | `dist/browser/spytial-core-complete.css` |
| `react-component-integration.global.js` | `dist/components/react-component-integration.global.js` |
| `react-component-integration.css` | `dist/components/react-component-integration.css` |
| `spytial-language.json` | `docs/spytial-language.json` |
| `spytial-spec.schema.json` | `docs/spytial-spec.schema.json` |
| `spytial-check.js` | `dist/cli/spytial-check.js` |

`.map` files are intentionally not vendored to keep the published crate small.

## `spytial-language.json`

Unlike the four browser assets, the manifest is never served to a browser. It is the
machine-readable description of the layout-spec language — every constraint and
directive, its fields, their closed vocabularies, numeric bounds, and which of them
the engine actually rejects versus silently ignores (`enforcement`).

`spec-codegen/` reads it to generate `macros/src/spec_tables.rs`, so the derive
macro's accepted keys and compile-time validation are derived from spytial-core's
own description of the language rather than transcribed by hand, and
`macros/src/attributes.md`, the attribute reference the derive's rustdoc includes
and the guide embeds. Bumping the vendored version and forgetting to regenerate is
caught by a test in that crate.

First shipped in spytial-core 4.3.0; there is no equivalent file in 4.1.0 or earlier.

## `spytial-spec.schema.json`

The JSON Schema for a layout spec, as spytial-core publishes it beside the manifest
(the release notes attach both). Like the manifest it is never served to a browser
and not read by the build: `tests/schema.rs` validates the YAML the derive emits
against it, which is the one check the engine cannot do itself — spytial-core's
parser ignores unknown keys silently, so a rule emitted under the wrong name or in
the wrong place renders a diagram quietly missing it. Small enough (under 30 KB) to
ship in the published crate, so the test runs from a `.crate` too.

First shipped beside the manifest in spytial-core 4.3.0.

## `spytial-check.js`

The conformance harness, used by `tests/conformance.rs`. Like the manifest it is
never served to a browser, and unlike the manifest it is not read by the build
either: the test shells out to it with `node`, writes a case document on stdin, and
parses the `RunResult` it writes back.

It lives here rather than under `tests/` so it moves with the same `VERSION.txt` pin
as everything else in this directory. A harness from one release checking specs
written against another is the failure it exists to prevent — `RunResult` carries a
`formatVersion` for the same reason, and the test asserts on it.

It is the one file here excluded from the published crate (see `Cargo.toml`): about
0.9 MB of code no consumer runs (it was 3.2 MB before spytial-core 5.2 trimmed it). `tests/conformance.rs`
skips when it is missing, so a `cargo test` from the published crate stays green.

The bundle is self-contained — a single file any Node ≥16 can run, with no
`node_modules` beside it. First shipped in spytial-core 4.4.1; 4.4.0 and earlier
have no `dist/cli` at all.

The query vocabulary it answers grows with the release, so the pin sets what the
tests can ask about. 4.4.2 added `cyclic()`, `sized()` and `hidden()`, which is what
lets `tests/conformance.rs` cover the `cyclic`, `size` and `hide_atom` decorators at
all. Downgrading below it is loud rather than silent: those queries come back as
`Unrecognized spatial query`, failing the case instead of passing vacuously.

## What we deliberately don't vendor

spytial-core 4.0.0 split two optional surfaces out of the CDN main global, each
into its own script: `dist/browser/spytial-core-sql.global.js` (`SQLEvaluator`) and
`dist/browser/spytial-core-explorer.global.js` (`<spytial-explorer>`). A page that
wants them loads the extra script after the main one.

`templates/template.html` needs neither: it evaluates selectors with
`SGraphQueryEvaluator`, and it renders through `<webcola-cnd-graph>`, which the main
global still registers. Adding either script would grow every generated page by
~0.4-0.5 MB for code the page never calls.
