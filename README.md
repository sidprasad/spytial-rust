# Spytial

[![crates.io](https://img.shields.io/crates/v/spytial.svg)](https://crates.io/crates/spytial)
[![docs.rs](https://img.shields.io/docsrs/spytial)](https://docs.rs/spytial)
[![PR checks](https://github.com/sidprasad/spytial-rust/actions/workflows/pr.yml/badge.svg)](https://github.com/sidprasad/spytial-rust/actions/workflows/pr.yml)
[![License](https://img.shields.io/crates/l/spytial.svg)](#license)

You can prove memory safety at compile time. You can derive `Hash`, `Ord`,
and `Serialize` from a single line. And yet, when you want to know the
shape of the `BTreeMap<NodeId, Vec<Edge>>` your program just built, you
reach for `dbg!` and start counting braces.

```text
[src/main.rs:42:17] tree = RBTree {
    root: Some(
        RBNode { key: 38, color: Black, left: Some(RBNode { key: 19,
        color: Black, left: Some(RBNode { key: 12, color: Black, left:
        Some(RBNode { key: 8, color: Red, left: None, right: None }),
        right: None }), right: Some(RBNode { key: 31, color: Red, ...
```

That's a red-black tree, but you have to rebuild it in your head from the
indentation.

Rust already knows how to walk the value — that's what `#[derive(Debug)]`
does. Spytial reuses that walk to draw a diagram instead of printing
nested text, through the same macro:

```diff
- std::dbg!(tree)
+ spytial::dbg!(tree)
```

Your terminal output is unchanged; a browser tab also opens with a diagram
of the value. `spytial::dbg!` takes the same arguments as `std::dbg!` and
returns the value the same way, so you can drop it in anywhere `std::dbg!`
already appears.

```rust
use spytial::{dbg, SpytialDecorators};
use serde::Serialize;

#[derive(Debug, Serialize, SpytialDecorators)]
#[attribute(field = "key")]
#[orientation(selector = "{x, y : Node | x->y in left}",  directions = ["left",  "below"])]
#[orientation(selector = "{x, y : Node | x->y in right}", directions = ["right", "below"])]
struct Node {
    key: u32,
    left: Option<Box<Node>>,
    right: Option<Box<Node>>,
}

let tree = build_tree();
let tree = dbg!(tree); // returns `tree` through; diagram opens in browser
```

Decorators describe *what should hold* about the layout, not *how to
render* it. They're declarative constraints, attached to the type once,
applied everywhere a value of that type appears. The
[progressive-refinement walkthrough](https://sidprasad.github.io/spytial-rust/decorators.html)
shows a flat graph turning into a red-black tree as you add constraints.

For the design philosophy and motivating examples, see Brown PLT's blog
post: [Diagramming Program Values by Spatial Refinement](https://blog.brownplt.org/2026/05/22/spytial.html).

## Install

```toml
[dependencies]
spytial = "0.1"
serde = { version = "1", features = ["derive"] }
```

## Reference

`dbg!` matches the `std::dbg!` calling convention:

| Form         | Behavior                                                          |
|--------------|-------------------------------------------------------------------|
| `dbg!()`     | Prints location to stderr (same as `std::dbg!()`)                 |
| `dbg!(x)`    | Prints `{:#?}` + opens diagram, returns `x` through               |
| `dbg!(&x)`   | Same, borrows                                                     |
| `dbg!(a, b)` | Returns `(a, b)`; one diagram tab per argument                    |

Type requirements: `Debug` (already required by `std::dbg!`), plus
`Serialize` and `SpytialDecorators`.

Environment variables:

| Variable               | Effect                                                          |
|------------------------|-----------------------------------------------------------------|
| `SPYTIAL_NO_OPEN=1`    | Skip browser launch; useful for `cargo test` and CI             |
| `SPYTIAL_OUTPUT_PATH`  | Pin the HTML output to a specific path (default: random tempfile) |

For library code, or anywhere you don't want stderr noise:

```rust
use spytial::diagram;
diagram(&tree); // no stderr, no source location, doesn't move
```

Failures (serialization, file write, missing browser) never panic — they
log a one-line warning to stderr and return. Use `try_export_json_instance`
for the fallible Result-returning path.

## Examples

```bash
cargo run --example dbg_basic     # smallest dbg! swap
cargo run --example demo          # decorator collection across nested structs
cargo run --example rbt           # progressive refinement of a red-black tree
```

## Headless / Docker

```bash
docker build -t spytial .
docker run --rm -p 8080:8080 spytial          # default: rbt
docker run --rm -p 8080:8080 spytial demo
```

Open `http://localhost:8080/rust_viz_data.html`. Browser launch is
disabled inside the container (`SPYTIAL_NO_OPEN=1`).

## Docs

- **Guide:** <https://sidprasad.github.io/spytial-rust/> — install, decorators, workflows, internals
- **API:** <https://docs.rs/spytial> — generated rustdoc
- **Design:** [Diagramming Program Values by Spatial Refinement](https://blog.brownplt.org/2026/05/22/spytial.html)

## Status

| | |
|---|---|
| Version | 0.1.0 |
| MSRV | Rust 1.80 |
| OS | macOS, Linux, Windows |
| License | MIT or Apache-2.0 |

## Contributing

See [CONTRIBUTING.md](./CONTRIBUTING.md). Bug reports and PRs welcome.

## License

Dual-licensed under [MIT](./LICENSE-MIT) or [Apache 2.0](./LICENSE-APACHE),
at your option.
