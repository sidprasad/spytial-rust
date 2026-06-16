# Getting started

## Install

Add spytial and serde to your `Cargo.toml`:

```toml
[dependencies]
spytial = "0.1"
serde = { version = "1", features = ["derive"] }
```

There's nothing else to install: no system packages, and nothing fetched
at runtime. The HTML template and the rendering JavaScript are compiled
into the crate as `include_str!` payloads, so diagrams work offline. The
minimum supported Rust version is **1.80**.

## Your first diagram

Drop this into `src/main.rs`:

```rust
use spytial::{dbg, SpytialDecorators};
use serde::Serialize;

#[derive(Debug, Serialize, SpytialDecorators)]
#[attribute(field = "key")]
struct Node {
    key: u32,
    left: Option<Box<Node>>,
    right: Option<Box<Node>>,
}

fn main() {
    let tree = Node {
        key: 5,
        left: Some(Box::new(Node { key: 3, left: None, right: None })),
        right: Some(Box::new(Node { key: 7, left: None, right: None })),
    };

    let _ = dbg!(tree);
}
```

Run it:

```sh
cargo run
```

Two things happen:

1. Your terminal prints `[src/main.rs:LINE:COL] tree = Node { … }` —
   exactly what `std::dbg!` would have printed.
2. A browser tab opens with the rendered tree.

The three derives are the whole contract: `Debug` (already required by
`std::dbg!`), plus `Serialize` and `SpytialDecorators`. The single
`#[attribute(field = "key")]` decorator promotes each node's `key` into
its label; without it the nodes would be anonymous. The next page,
[Decorators](./decorators.md), covers the rest.

## The two entry points

| Call | Behavior |
|------|----------|
| `dbg!(x)` | Prints `[file:line:col] x = {:#?}` to stderr, opens a diagram, returns `x` through. `dbg!(&x)` borrows; `dbg!(a, b)` returns `(a, b)` and opens one tab per argument. |
| `diagram(&x)` | No stderr, no source location, doesn't move `x`. Use it in library code or anywhere you don't want debug noise. |

Decorators on a type apply automatically wherever a value of that type
appears inside another decorated type — the derive walks `Vec<T>`,
`Option<T>`, `Box<T>`, and their nested combinations at compile time. You
never register nested types anywhere.

## Where the file lives

By default spytial writes each diagram to a unique file in your OS temp
directory, named like `spytial-{pid}-{counter}-{nanos}.html`, so
concurrent `dbg!` calls don't trample each other.

To pin the output to a known path — for serving it from a static file
server, or copying it off a remote machine — set `SPYTIAL_OUTPUT_PATH`:

```sh
SPYTIAL_OUTPUT_PATH=/tmp/my-diagram.html cargo run
```

The path is taken verbatim and overwritten on each call, so this is a
one-diagram-at-a-time setup.

## Skipping the browser launch

In CI, over SSH, or inside a container — anywhere there's no display —
disable the browser launch with `SPYTIAL_NO_OPEN` (`1`, `true`, or `yes`):

```sh
SPYTIAL_NO_OPEN=1 cargo run
```

stderr is unaffected, so `cargo test` capture behaves exactly as it does
for `std::dbg!`. With the launch suppressed, spytial prints the path of
the rendered HTML so you can open it manually. See
[Running headless & in Docker](./headless.md) for the full setup.
