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
- **They're collected transitively at compile time.** Decorating `Person`
  is enough for those rules to apply wherever `Person` appears inside a
  `Vec<T>`, `Option<T>`, `Box<T>`, or their nested combinations.

## A red-black tree, one rule at a time

The default layout gets you most of the way for trees and lists; a few
decorators do the rest. This is the full
[`examples/rbt.rs`](https://github.com/sidprasad/spytial-rust/blob/main/examples/rbt.rs)
demo, built up stage by stage — each snippet is an attribute you add to
the struct.

**Stage 1 — bare derive.** Three derives, no decorators. The diagram is a
correct but flat graph; you can't read it as a tree yet.

```rust
#[derive(Debug, Serialize, SpytialDecorators)]
struct RBNode {
    key: u32,
    color: Color,
    left: Option<Box<RBNode>>,
    right: Option<Box<RBNode>>,
}
```

**Stage 2 — show the key.** Each `RBNode` atom now carries its key as a
label, so you can see what's where.

```rust
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
#[atom_color(selector = "{x : RBNode | @:(x.color) = Red}",   value = "red")]
#[atom_color(selector = "{x : RBNode | @:(x.color) = Black}", value = "black")]
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
| `#[attribute(field = "...")]` | Promote a field's value into the node's label. |
| `#[flag(name = "...")]` | Set a global display flag, e.g. `hideDisconnected`. |

### Layout constraints

| Attribute | What it does |
|-----------|--------------|
| `#[orientation(selector = "...", directions = [...])]` | Place matched pairs in a direction. `directions` ⊆ `"left"`, `"right"`, `"above"`, `"below"`. |
| `#[align(selector = "...", direction = "horizontal" \| "vertical")]` | Force matched atoms to share an axis. |
| `#[cyclic(selector = "...", direction = "clockwise" \| "counterclockwise")]` | Arrange matched atoms around a ring. |
| `#[group(...)]` | Cluster related atoms into a labelled region — by `field` or by `selector` (the two are mutually exclusive; if `field` is present the selector is ignored). |

`orientation`, `align`, `cyclic`, and `group` each take an optional
`negated = true` — see [Negated constraints](#negated-constraints) below.

### Styling, filtering, and overrides

| Attribute | What it does |
|-----------|--------------|
| `#[atom_color(selector = "...", value = "...")]` | Color matched atoms (`value` is any CSS color). |
| `#[size(selector = "...", height = ..., width = ...)]` | Override node dimensions (in diagram units). |
| `#[icon(selector = "...", path = "...", show_labels = ...)]` | Replace matched atoms with an image icon (`path` is a path or URL). |
| `#[edge_style(field = "...", value = "...", style = "...", ...)]` | Style relation arrows (color, `solid`/`dashed`, weight, label, hidden). |
| `#[projection(sig = "...")]` | Project atoms of `sig` out of the main view. |
| `#[hide_field(field = "...")]` | Suppress a relation from the rendering. |
| `#[hide_atom(selector = "...")]` | Suppress matched atoms entirely. |
| `#[inferred_edge(name = "...", selector = "...")]` | Define a synthetic edge derivable from the data. |
| `#[tag(to_tag = "...", name = "...", value = "...")]` | Attach a computed attribute to matched atoms. |

These map onto the same builder and YAML layer that spytial-core consumes,
so anything expressible here is also expressible as a hand-written spec
passed to [`diagram_with_spec`](./library.md).

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
