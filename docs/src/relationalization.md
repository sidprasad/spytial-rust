# Relationalization and reification

Spytial converts a Serde value into a relational instance for the diagram
renderer. The forward operation is **relationalization**. The reverse operation,
**reification**, reconstructs a Rust value of a specified type from that
instance.

This page describes the representation produced by
`try_export_json_instance(&value)` and read by `from_datum::<T>(&instance)`.
The input is the [Serde data model](https://serde.rs/data-model.html), which is
the structure exposed by a type's `Serialize` implementation. A Rust type may
choose a Serde representation that differs from its Rust syntax.

## The relational instance

An instance has two lists:

```text
JsonDataInstance {
    atoms:     [IAtom { id, type, label }, ...],
    relations: [IRelation { id, name, types, tuples }, ...],
}
```

An **atom** represents one serialized value or one container occurrence. Its
`id` is unique within the instance. Its `type` is a Serde type name, a named
struct or enum type, or one of Spytial's container names. Its `label` is text
for display and, for scalar values, for reconstruction. An atom is created for
each primitive value, string, byte array, unit value, `None`, named value, and
sequence, tuple, or map container. Sequence positions are also integer atoms.

A **relation tuple** connects atoms. Its first position is the source atom.
The remaining positions are atoms that describe a part of that source. Spytial
uses three principal forms:

```text
field_name(parent, value)       // a struct or struct-variant field
idx(container, position, value) // a sequence or positional field
map_entry(map, key, value)      // a map entry
```

Formally, let `E(x)` be the atom ID returned when the serializer visits `x`.
For a scalar, `E(x)` identifies its value atom. For a composite value, the
serializer creates a parent atom, recursively obtains `E` for each part, and
adds the corresponding tuples above. The return value is the parent atom ID.
`Option` is the exception: an ordinary `Some(x)` returns `E(x)` without a new
parent atom, as specified in the table below.

The `value`, `position`, and `key` positions refer to atom IDs, even when a
diagram displays their labels. A field name is a relation name because it is a
fixed role supplied by Serde. A map key is an atom because it is data supplied
at run time. Empty containers have atoms but no element tuples.

An `IRelation` groups tuples by **source atom type and relation name**. For
example, `Person.name` and `Company.name` are separate records, both named
`name`. An `ITuple` contains parallel `atoms` and `types` lists. The record's
`types` is a summary of its tuples: a position with differing types becomes
`atom`, and a mixed-arity record retains the longest arity. The tuple's own
`types` remains exact. A dot or backslash in either part of a relation ID is
escaped with a backslash.

The following example shows the difference between a container atom, an
element atom, and a relation. The two equal integers use one atom:

```rust
#[derive(serde::Serialize)]
struct Sample { values: Vec<i32> }

let value = Sample { values: vec![7, 7] };
```

```text
atoms:
  s  = (type Sample,   label Sample)
  v  = (type sequence, label seq[2])
  n  = (type i32,      label 7)
  p0 = (type u64,      label 0)
  p1 = (type u64,      label 1)

relation tuples:
  Sample.values(s, v)
  sequence.idx(v, p0, n)
  sequence.idx(v, p1, n)
```

The symbolic IDs above explain the structure; actual export IDs have the form
`atom0`, `atom1`, and so on. Equal scalar values of the same exposed type
normally share an atom. Floats are keyed by bit pattern, so `0.0` and `-0.0`
remain distinct; NaN occurrences are not shared. Unit values, unit structs,
unit variants, and `None` are shared by their type and label. Other composite values
receive separate atoms for separate serialized occurrences. The instance does
not record Rust pointer identity or aliasing.

## Every Serde category

The table covers all 29 categories in the Serde data model. “Reverse” means
how `from_datum::<T>` reads an instance produced by this serializer. It needs
the target type `T`; the graph alone does not specify a Rust type.

| Serde category | Forward: atoms and relation tuples | Reverse |
| --- | --- | --- |
| `bool` | One `bool` atom labeled `true` or `false`. | Parse the label as `bool`. |
| `i8`, `i16`, `i32`, `i64`, `i128` | One atom of the corresponding signed type, with a decimal label. | Parse the label at the requested width. |
| `u8`, `u16`, `u32`, `u64`, `u128` | One atom of the corresponding unsigned type, with a decimal label. | Parse the label at the requested width. |
| `f32`, `f64` | One atom of the corresponding float type, with a textual float label. Non-NaN values are shared by bit pattern. | Parse the label at the requested width. |
| `char` | One `char` atom labeled with the character itself. | Read the character from the label. |
| `string` | One `string` atom labeled with the raw UTF-8 text. | Supply that text to Serde's string visitor. |
| `byte array` | One `bytes` atom labeled like `[0, 255]`. | Parse the list of decimal bytes from the label. |
| `option` | `None` is one `None` atom. `Some(x)` normally uses the atom for `x` directly. If `x` is itself a `None` or `Some` atom, a `Some` atom and `value(Some, x)` preserve the nesting. | A `None` atom means absent; a `Some` atom follows `value`; any other atom is treated as an unwrapped `Some(x)`. |
| `unit` | One `unit` atom labeled `()`. | Visit unit. |
| `unit_struct` | One `unit_struct` atom labeled with the struct name. | Visit unit for the requested struct type. |
| `unit_variant` | One atom whose type is the enum name and whose label is the variant name. | Select the variant from the label; it has no payload. |
| `newtype_struct` | One `newtype_struct` atom labeled with its name, plus `value(wrapper, inner)`. | Follow `value` and deserialize the inner value. |
| `newtype_variant` | One enum-type atom labeled with the variant name, plus `variant_value(variant, inner)`. | Select the variant and follow `variant_value`. |
| `seq` | One `sequence` atom and `idx(sequence, position, element)` for each element. | Sort by the integer position and visit elements in order. |
| `tuple` | One `tuple` atom and one `idx` tuple per element. | Read positional elements in order. |
| `tuple_struct` | One `tuple_struct` atom labeled with its name and one `idx` tuple per field. | Read positional fields in order for the requested type. |
| `tuple_variant` | One enum-type atom labeled with the variant name and one `idx` tuple per field. | Select the variant, then read positional fields in order. |
| `map` | One `map` atom and `map_entry(map, key, value)` for each entry. Keys and values are both atoms and may themselves be containers. | Visit the key and value of each entry. |
| `struct` | One atom whose type and label are the struct name; each serialized field `f` adds `f(struct, value)`. | Supply relation names as field names to the requested struct's `Deserialize` implementation. |
| `struct_variant` | One enum-type atom labeled with the variant name; each serialized field `f` adds `f(variant, value)`. | Select the variant, then supply its field relations. |

The exporter serializes position atoms as `u64`. A sequence or map
whose length is unknown when serialization begins is labeled `seq[0]` or
`map[0]`; that label is only display text. Reification obtains elements from
relations, not from the label. The category is chosen by the type's `Serialize`
implementation: an ordinary `Vec<u8>` can use `seq`, while a byte-buffer
implementation calls Serde's `serialize_bytes` and uses `byte array`.

## The reverse operation

For a value `v` of type `T`, the intended round trip is:

```rust
use spytial::{from_datum, replit};
use spytial::export::try_export_json_instance;

let instance = try_export_json_instance(&v)?;
let reconstructed: T = from_datum(&instance)?;
let debug_text = replit::<T>(&instance)?;
```

`from_datum` builds an index from atom IDs and outgoing relation tuples. It
chooses the first atom with no incoming relation as the root, then drives
`T::deserialize` from that atom. `from_datum_root` accepts an explicit root ID
for a caller-supplied or reordered instance. Root selection is based on
relations because a nested `Some` wrapper is emitted *after* its inner atom;
therefore `atoms[0]` is not always the root. The exported shape has no `rootId`
field.

`replit::<T>` reconstructs a real `T` and applies that type's `Debug`
implementation. It does not print atom labels as a substitute for `Debug`.
The evaluation corpus checks two properties where each is meaningful:

```text
R-eq:      v == from_datum::<T>(&export(v))
R-inspect: format!("{:?}", v) == replit::<T>(&export(v))
```

These are properties of compatible `Serialize` and `Deserialize`
implementations. The serializer does not make arbitrary custom implementations
inverse to each other.

## Conventions and limits

- **Serde can omit information.** `#[serde(skip)]` removes a field from the
  instance. On reconstruction, `Deserialize` supplies its default, which can
  change the value and its `Debug` output. `skip_serializing_if` can round trip
  when the omitted value is recoverable through a matching default. A custom
  lossy `Serialize` implementation has the same general limit.
- **Some Serde representations need self-description.** `flatten`, `untagged`,
  and internally or adjacently tagged enums can call `deserialize_any` before
  the target shape is known. Reification then infers a shape from the atom type,
  label, and outgoing relations. In this path, a user type named like a built-in
  atom type (such as `map`), a unit variant whose name equals its enum name,
  and a one-field struct variant whose only field is `variant_value` are
  ambiguous. The last shape is indistinguishable from a newtype variant in
  the exported instance. A zero-field struct variant or zero-element tuple
  variant has no outgoing relation and is indistinguishable from a unit
  variant. These empty variants fail when reached through `deserialize_any`,
  for example inside an untagged enum.
- **Labels serve as data for scalar reconstruction.** Byte arrays are stored
  as decimal-list labels and parsed back. Floats are keyed by bits during
  export, but reification parses their labels. It does not preserve a NaN's
  payload bits. `NaN` cannot satisfy `R-eq` because it is unequal to itself;
  the corpus checks its `Debug` representation instead.
- **Order and identity have limits.** `idx` preserves sequence order. Map
  entries follow serialization order, which need not be stable for `HashMap`.
  A reconstructed `HashMap` may print entries in a different order, so the
  corpus checks `R-eq` for it, not exact `Debug` text. Repeated composite values
  and shared pointers are serialized as separate occurrences. Pointer cycles
  are outside this tree representation. A graph whose edges are stored as
  ordinary indices can still round trip.
- **The reverse reader expects export-shaped input.** It uses field relation
  names, `idx`, `map_entry`, `value`, and `variant_value` according to the
  requested type. It does not validate an arbitrary edited relational graph
  as a complete, unambiguous schema.

The [library API](./library.md) describes the export and reification entry
points. The repository's `eval-corpus` suite exercises every Serde category
and the representation cases above.
