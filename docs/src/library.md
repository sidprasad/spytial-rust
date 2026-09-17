# Library API

The `dbg!` macro and `diagram()` are tuned for interactive use: `dbg!`
pretty-prints the value to stderr and appends it to the process viewer,
`diagram()` opens a standalone page, and both swallow errors with a one-line
`eprintln!`. When spytial is buried inside something else — a CLI
subcommand, a test harness, a service that renders diagrams for clients —
you'll want the lower-level entry points instead.

## `ViewerSession` and `dbg_in!` — separate capture streams

```rust
use spytial::{dbg_in, ViewerSession};

let parser = ViewerSession::named("parser");
let (before, after) = dbg_in!(&parser; before, after);
eprintln!("session snapshot: {}", parser.output_path().display());
```

`dbg!` uses one lazily-created process session. `ViewerSession::new()` and
`ViewerSession::named()` create independent sessions; `dbg_in!` provides the
same evaluation, stderr, ownership, and return-value behavior while routing
captures to one of them. `ViewerSession::diagram()` adds a capture without the
`Debug` print and labels its expression `diagram`.

Each session starts its browser and loopback transport at most once, on the
first non-headless capture. Cloning `ViewerSession` shares the capture stream,
which makes it safe to pass to worker threads. `id()`, `name()`, and
`output_path()` expose the session metadata without exposing the transport.

## `diagram(&value)` — render and open

```rust
use spytial::diagram;
diagram(&tree);
```

Renders the value, writes to a temp file, opens a browser tab. Every step
is best-effort: any failure is printed to stderr and swallowed, and the
function returns `()`. No source location, and it borrows rather than
moves. Any `Serialize` value works. A `SpytialDecorators` derive is detected
automatically when present, but is not required.

Automatic discovery comes from the derive's link-time registration. If you
implement `HasSpytialDecorators` by hand, `diagram()` cannot conditionally
detect that trait implementation on stable Rust. Call `T::decorators()`, encode
it with `spytial_annotations::to_yaml`, and pass it to `diagram_with_spec`
instead.

Rules assembled through `SpytialDecoratorsBuilder` carry no `source` unless
you stamp one: `.source("#[orientation(...)]", Some("src/tree.rs:14"))` marks
the rule pushed just before it, and that text is what the viewer cites in a
conflict report. The derive does this for every attribute automatically.

## `diagram_with_spec(&value, spec)` — hand-written constraints

```rust
use spytial::diagram_with_spec;

let spec = r#"
constraints:
  - align:
      selector: reports_to
      direction: horizontal
directives:
  - flag: hideDisconnected
"#;

diagram_with_spec(&tree, spec);
```

Same diagram-and-browser flow, but with a YAML spec you assemble yourself,
bypassing any derive-generated decorators on `T`. Useful for adding rules to
a standard-library or third-party type, overriding the derive output for one
call, or generating the spec from configuration. The YAML schema is the one
the `SpytialDecorators` derive emits — see [Decorators](./decorators.md).

## `export_json_instance(&value)` — capture the data, render nothing

```rust
use spytial::export_json_instance;
let instance = export_json_instance(&tree); // JsonDataInstance { atoms, relations }
```

Returns the relational representation of the value without writing HTML or
touching the browser — the right entry point for persisting diagram data,
sending it to a remote renderer, or feeding a tool with its own UI. It's
infallible at the call boundary: if serialization fails it logs to stderr
and returns an empty instance.

See [Relationalization and reification](./relationalization.md) for the atom
and relation format, its mapping from Serde values, and the reverse operation.

## `try_export_json_instance(&value) -> Result<…>` — fallible export

```rust
use spytial::export::try_export_json_instance;

match try_export_json_instance(&tree) {
    Ok(instance) => persist(instance),
    Err(err) => eprintln!("spytial export failed: {err}"),
}
```

The fallible variant: a `Serialize` error comes back to you instead of a
silent empty instance. **This is the right choice for library code** that
wants to surface failure to its caller, and for tests that assert
serialization succeeded.

## `from_datum::<T>(&instance)` and `replit::<T>(&instance)` — reconstruct

`from_datum` uses `T: DeserializeOwned` to rebuild a Rust value from an
exported instance. `replit` also requires `Debug` and returns the rebuilt
value's `{:?}` string. Use `from_datum_root` or `replit_root` when the root atom
ID is known explicitly. Reconstruction requires a compatible
`Deserialize` implementation; see [Relationalization and
reification](./relationalization.md) for its precise scope and limits.

## Choosing between them

| Use case | Entry point |
|----------|-------------|
| Ad-hoc debugging with stderr trail | `spytial::dbg!` |
| Named or separate debug capture stream | `dbg_in!(&session; value)` |
| Capture into an explicit session without stderr | `ViewerSession::diagram(&value)` |
| One-call render with auto layout | `diagram(&value)` |
| Render with a custom YAML spec | `diagram_with_spec(&value, spec)` |
| Capture relational JSON only | `export_json_instance(&value)` |
| Same, but surface errors | `try_export_json_instance(&value)` |
| Rebuild a typed value | `from_datum::<T>(&instance)` |
| Rebuild and print with `Debug` | `replit::<T>(&instance)` |
