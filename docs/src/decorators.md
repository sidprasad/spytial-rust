# Decorators

Spytial decorators describe *what should hold* about a layout — not *how
to render* it. They're declarative constraints, attached to a type once
with an attribute, applied everywhere a value of that type appears.

A decorator like `#[align(selector = "reports_to", direction = "horizontal")]`
does not say "draw an arrow from A to B." It says "atoms related by
`reports_to` should line up horizontally." The layout engine picks the
actual coordinates, as long as the constraint holds. Three things follow:

- **They compose.** Add an orientation rule, then a color, then a hide
  rule — the diagram refines step by step. No rule has to know about any
  other.
- **They're per-type, not per-instance.** Decorate `RBNode` once; every
  `RBNode` in the value picks up the same rules, however deeply nested.
- **They're registered and collected transitively.** Decorating `Person`
  is enough for those rules to apply wherever `Person` appears inside a
  `Vec<T>`, `Option<T>`, `Box<T>`, or their nested combinations.

## A red-black tree, one rule at a time

The default layout gets you most of the way for trees and lists; a few
decorators do the rest. This is the full
[`examples/rbt.rs`](https://github.com/sidprasad/spytial-rust/blob/main/examples/rbt.rs)
demo, built up stage by stage — each snippet is an attribute you add to
the struct.

**Stage 1 — no decorators.** `Debug + Serialize` is enough for a correct but
flat structural graph; you can't read it as a tree yet.

```rust
#[derive(Debug, Serialize)]
struct RBNode {
    key: u32,
    color: Color,
    left: Option<Box<RBNode>>,
    right: Option<Box<RBNode>>,
}
```

**Stage 2 — show the key.** Add the derive and the first decorator. Each
`RBNode` atom now carries its key as a label, so you can see what's where.

```rust
#[derive(Debug, Serialize, SpytialDecorators)]
#[attribute(field = "key")]
```

**Stage 3 — make it a tree.** Left children go down-and-left, right
children down-and-right; the layout now encodes BST order.

```rust
#[orientation(selector = "{x, y : RBNode | x->y in left}",  directions = ["left",  "below"])]
#[orientation(selector = "{x, y : RBNode | x->y in right}", directions = ["right", "below"])]
```

The selector reads: "for any pair `(x, y)` of `RBNode`s where `x -> y` is
in the `left` relation, place `y` to the left of and below `x`."

**Stage 4 — make it a *red-black* tree.** Color nodes by their `color`
field. The pattern matches *any* matching node, not specific instances.

```rust
#[atom_style(selector = "{x : RBNode | @:(x.color) = \"Red\"}",   border_style(color = "red"))]
#[atom_style(selector = "{x : RBNode | @:(x.color) = \"Black\"}", border_style(color = "black"))]
```

`\"Red\"` is a string literal in the query, escaped because the selector is
itself a Rust string. Quoting matters: a bare name that resolves to nothing
is the empty relation, so the comparison would just be false and the rule
would quietly never fire — the diagram still renders, only unstyled. The
diagram flags this with a `⚠ n selector warnings` bar naming the decorator
and its selector.

A raw string carries the same query without the backslashes, if you prefer
to read the selector as the query engine sees it:

```rust
#[atom_style(selector = r#"{x : RBNode | @:(x.color) = "Red"}"#, border_style(color = "red"))]
```

**Stage 5 — hide the scaffolding.** The `Color` enum atoms, the `u32`
keys, and the `None` sentinels are already implied by node color, labels,
and absent edges. Drop them from the canvas.

```rust
#[hide_atom(selector = "Color + u32 + None")]
```

`Color + u32 + None` is a set expression: match any atom whose type is
`Color`, `u32`, or the literal `None`. Each rule refined an existing
structure rather than imposing an external aesthetic.

## Attribute reference

Every decorator is a Rust attribute on a type that derives
`SpytialDecorators`. They group into three families.

### Display

| Attribute | What it does |
|-----------|--------------|
| `#[attribute(field = "...", selector = "...", filter = "...")]` | Promote a field's value into the node's label. |
| `#[flag(name = "...")]` | Set a global display flag. `name` is required and is one of `hideDisconnected`, `hideDisconnectedBuiltIns`. |

### Layout constraints

| Attribute | What it does |
|-----------|--------------|
| `#[orientation(selector = "...", directions = [...])]` | Place matched pairs in a direction. Required; each value is one of `"above"`, `"below"`, `"left"`, `"right"`, or a `"directly*"` variant. `above`/`below` and `left`/`right` are mutually exclusive, and a `directly*` value admits only its own plain counterpart alongside it. |
| `#[align(selector = "...", direction = "horizontal" \| "vertical")]` | Force matched atoms to share an axis. |
| `#[cyclic(selector = "...", direction = "clockwise" \| "counterclockwise")]` | Arrange matched atoms around a ring. `direction` defaults to `clockwise`. |
| `#[group(selector = "...", name = "...", show_label = ...)]` | Cluster related atoms into a region. A binary selector builds one group per distinct first-column atom, keyed by it; a unary selector builds a single unkeyed group. Set `show_label = false` to hide the group caption and its background pill while keeping the hull. |

`orientation`, `align`, `cyclic`, and `group` each take an optional
`negated = true` — see [Negated constraints](#negated-constraints) below.

### Styling, filtering, and overrides

Styling follows spytial-core's block system, introduced in 3.x. Blocks are
written as nested groups that mirror the YAML 1:1:

- `line_style(color = "...", pattern = "solid" | "dashed" | "dotted", weight = 2.0, highlight = "...")` — a drawn edge line;
- `text_style(size = "small" | "normal" | "large", color = "...")` — any label;
- `border_style(color = "...", width = 2.0)` / `fill_style(color = "...")` — an atom's outline and interior;
- `icon_style(path = "...", placement = "full" | "badge", opacity = 0.4)` — an atom's icon.

Every block field is optional — set only what you mean.

Which keys each attribute accepts, and which values are legal, are generated
from spytial-core's own language manifest (see `spec-codegen/`). A key or value
this crate rejects is one spytial-core would reject or silently ignore, so
typos, unknown block leaves, out-of-vocabulary values, and out-of-range numbers
are all compile errors rather than a diagram that renders without them.

| Attribute | What it does |
|-----------|--------------|
| `#[atom_style(selector = "...", border_style(...), fill_style(...), icon_style(...), text_style(...), show_label = ...)]` | Style matched atoms' border, interior fill, icon, and label independently. |
| `#[size(selector = "...", height = ..., width = ...)]` | Override node dimensions (in diagram units). |
| `#[icon(selector = "...", path = "...", show_labels = ...)]` | Deprecated by spytial-core 4.2; rewrites to `atom_style`. Its single `show_labels` boolean drove label visibility and icon geometry at once — prefer `icon_style(...)` plus `show_label`, which are independent. |
| `#[edge_style(field = "...", line_style(...), text_style(...), show_label = ..., hidden = ...)]` | Style relation arrows: the drawn line, the edge's label, and visibility. |
| `#[hide_field(field = "...", selector = "...", filter = "...")]` | Suppress a relation from the rendering. |
| `#[hide_atom(selector = "...")]` | Suppress matched atoms entirely. |
| `#[inferred_edge(name = "...", selector = "...", draw = "...", line_style(...), text_style(...))]` | Define a synthetic edge derivable from the data, optionally styled. `draw` attaches its ends to group hulls — see below. |
| `#[tag(to_tag = "...", name = "...", value = "...", text_style(...))]` | Attach a computed attribute to matched atoms. |
| `#[attribute(field = "...", text_style(...))]` | (See Display above; `text_style` styles the attribute's line.) |

An `inferred_edge` additionally takes `draw = "<end> -> <end>"`, which decides
what each end of the edge *attaches* to. Each end is either `_` — the tuple's
own atom, which is what an edge without `draw` does — or the name of a `group`
constraint, in which case the end lands on that group's hull:

```rust
#[group(selector = "region", name = "regions")]
// One edge per `connected` pair, drawn hull to hull.
#[inferred_edge(name = "connected", selector = "connected", draw = "regions -> regions")]
// Each person to the hull of the region-group they manage.
#[inferred_edge(name = "manages", selector = "manages", draw = "_ -> regions")]
```

`draw` never decides *which* pairs get edges or which way they point — the
selector does (transpose it, e.g. `~connected`, to flip one). A keyed group
constraint builds one group per key and the end's atom picks which; a unary one
builds a single group the end attaches to directly. With `draw`, the selector
may also be unary: the single atom feeds both ends.

Malformed `draw` strings are compile errors, including the redundant
`"_ -> _"` (that's the default — drop the key). The group *name*, though, is
resolved by spytial-core against the assembled spec: decorators compose
across types, so no single attribute site can see which `group` constraints
will end up in the spec. A name that no `group` defines is a layout-time
warning (since spytial-core 5.1.0) and that edge is skipped; the rest of the
diagram still renders.

A selector-based `group` additionally takes `add_edge` — the connector between
the group's key and the group. Bare form `add_edge = "togroup"` (or
`"fromgroup"`/`"none"`), or the styled block
`add_edge(points = "togroup", line_style(...), text_style(...))` — plus a
top-level `text_style(color = "...")` for the group's own label.

**Migrating from the 2.x flat forms:** `#[atom_color(selector, value)]` and
`#[edge_style(field, value, style, weight, ...)]` still compile and are
rewritten onto the block forms — `atom_color`'s `value` becomes the *border*
colour (that is what 2.x drew; reach for `fill_style` only if you want a
filled look), and `edge_style`'s `value`/`style`/`weight` become
`line_style`'s `color`/`pattern`/`weight`. Mixing flat keys and blocks in one
`edge_style` is a compile error.

**Deprecation warnings:** every form spytial-core has deprecated now warns at
compile time, naming the replacement and how the fields map across. That covers
whole attributes (`#[atom_color]`, `#[icon]`) and single shapes of attributes
that are otherwise current — `#[edge_style(value = ...)]` warns while
`#[edge_style(field = ..., line_style(...))]` does not. Nothing stops
compiling; the deprecated forms still work exactly as before. To keep one
deliberately, put `#[allow(deprecated)]` on the type:

```rust,ignore
#[derive(Serialize, SpytialDecorators)]
#[allow(deprecated)]
#[atom_color(selector = "Node", value = "red")]
struct Node { /* ... */ }
```

The warning text comes from spytial-core's own language manifest, so it moves
when upstream's does.

> **Breaking in spytial-core 3.0:** two style rules that set the same property
> of the same edge/atom to *different* values now raise a
> `StyleCollisionError` at render time (2.x silently kept the first). Set each
> property in exactly one matching rule.

These map onto the same builder and YAML layer that spytial-core consumes,
so anything expressible here is also expressible as a hand-written spec
passed to [`diagram_with_spec`](./library.md).

## Where a rule came from

Every rule the derive emits is stamped with a `source` block: the attribute
exactly as you wrote it and its `file:line`. When the viewer reports a
conflict — two rules that cannot both hold — or warns about a rule that
matched nothing, it cites that text in place of its own description of the
rule, so the report reads `#[orientation(selector = "…", directions = ["left",
"below"])] at src/tree.rs:14` and you can go straight to the line. Nothing to
enable; a hand-written spec passed to `diagram_with_spec` needs no `source`,
because there the YAML is what you wrote. The builder's `.source(text,
location)` stamps the rule you pushed last, if you assemble decorators by hand
and want the same treatment.

## Attribute reference

The reference below is generated from spytial-core's own language manifest,
the same file the derive's compile-time checks come from, so the keys,
vocabularies, bounds, defaults, and selector arities are the engine's
description of the language at the vendored version. It is the same text
[docs.rs](https://docs.rs/spytial/latest/spytial/derive.SpytialDecorators.html)
shows on the derive.

{{#include ../../macros/src/attributes.md}}

## Negated constraints

`orientation`, `align`, `cyclic`, and `group` accept `negated = true`,
which flips the constraint from "this *must* hold" to "this *must not*
hold." It's most useful alongside positive constraints — the positive ones
say what shape the layout should take, the negated ones rule out a
degenerate arrangement the solver might otherwise pick:

```rust
#[align(selector = "{x, y : Node | x->y in default_edge}", direction = "vertical")]
#[align(selector = "{x, y : Node | x->y in exception_edge}", direction = "vertical", negated = true)]
```

"Default edges align vertically; exception edges must not." A diagram with
*only* negated constraints is under-specified, and the solver falls back to
defaults.
