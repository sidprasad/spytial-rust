# Spytial for Rust

**See the shape of your data, not the braces.** Spytial is a drop-in
replacement for `std::dbg!` that opens an interactive diagram of a Rust
value in your browser.

When you want to know the shape of the tree your program just built, you
reach for `dbg!` and start counting braces:

```text
[src/main.rs:42:17] tree = RBTree {
    root: Some(RBNode { key: 38, color: Black, left: Some(RBNode {
    key: 19, color: Black, left: Some(RBNode { key: 12, color: Black,
    left: Some(RBNode { key: 8, color: Red, left: None, ...
```

You know what this *is*. You'd sketch it on paper in five seconds. The
terminal won't. Spytial draws it for you — change one word:

```diff
- std::dbg!(tree)
+ spytial::dbg!(tree)
```

stderr stays identical. A browser tab opens with an interactive diagram of
the same value. `spytial::dbg!` is a strict superset of `std::dbg!` —
same calling convention, same return value, plus the picture.

## Run it now

Add spytial to a project (`cargo add spytial serde --features serde/derive`),
then:

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

`cargo run` prints the usual `dbg!` line to your terminal and opens the
tree in your browser. That's the whole loop. Walk through it step by step
in [Getting started](./getting-started.md).

## Shape the layout with decorators

The `#[attribute]` above is a *decorator*: a declarative rule attached to
a type that says what should hold about the layout, not how to draw it.
Add a few more and a flat graph clarifies into the picture you'd sketch —
left children down-and-left, nodes colored by a field, scaffolding hidden:

```rust
#[orientation(selector = "{x, y : Node | x->y in left}",  directions = ["left",  "below"])]
#[orientation(selector = "{x, y : Node | x->y in right}", directions = ["right", "below"])]
```

Decorators are collected at compile time across nested types, so you
decorate `Node` once and every `Node` in the value picks up the rules.
See [Decorators](./decorators.md) for the full reference and a red-black
tree built up one rule at a time.

## How it works

Three steps, all baked into the crate — nothing phones home and diagrams
work offline:

1. `#[derive(SpytialDecorators)]` collects your decorators at compile time.
2. `diagram(&value)` walks the value through serde into atoms and relations.
3. That data and your decorators fill a self-contained HTML template,
   rendered in the browser by a bundled copy of
   [spytial-core](https://github.com/sidprasad/spytial-core).

Failures never panic: a serialization error, a missing temp dir, or no
browser to open all log a one-line warning and return. `dbg!(x)` always
returns `x`.

## Where next

- [Getting started](./getting-started.md) — install, your first diagram, and where the output goes.
- [Decorators](./decorators.md) — every layout rule, plus a worked red-black tree.
- [Running headless & in Docker](./headless.md) — CI, containers, no-display environments.
- [Library API](./library.md) — using spytial as a dependency instead of a debug macro.

The longer design argument is on Brown PLT's blog:
[Diagramming Program Values by Spatial Refinement](https://blog.brownplt.org/2026/05/22/spytial.html).
Generated rustdoc is on [docs.rs](https://docs.rs/spytial).
