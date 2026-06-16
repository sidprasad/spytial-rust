# Library API

The `dbg!` macro and `diagram()` are tuned for interactive use: `dbg!`
pretty-prints the value to stderr, both open a browser, and both swallow
errors with a one-line `eprintln!`. When spytial is buried inside
something else — a CLI
subcommand, a test harness, a service that renders diagrams for clients —
you'll want the lower-level entry points instead.

## `diagram(&value)` — render and open

```rust
use spytial::diagram;
diagram(&tree);
```

Renders the value, writes to a temp file, opens a browser tab. Every step
is best-effort: any failure is printed to stderr and swallowed, and the
function returns `()`. No source location, and it borrows rather than
moves.

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
bypassing the derive-generated decorators on `T`. Useful for a type you
can't add a derive to, for overriding the derive output for one call, or
for generating the spec from configuration. The YAML schema is the one the
`SpytialDecorators` derive emits — see [Decorators](./decorators.md).

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

## Choosing between them

| Use case | Entry point |
|----------|-------------|
| Ad-hoc debugging with stderr trail | `spytial::dbg!` |
| One-call render with auto layout | `diagram(&value)` |
| Render with a custom YAML spec | `diagram_with_spec(&value, spec)` |
| Capture relational JSON only | `export_json_instance(&value)` |
| Same, but surface errors | `try_export_json_instance(&value)` |
