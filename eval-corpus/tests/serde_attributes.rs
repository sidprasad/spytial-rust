//! The corpus's second axis: serde's *representation* attributes.
//!
//! `serde_data_model.rs` covers the 29 structural categories and `pbt.rs`
//! nests them at random. Neither reaches this: `#[serde(flatten)]`,
//! `#[serde(untagged)]` and the tagged enum forms add no new category, they
//! change how an existing one is written down. They matter here because all
//! of them ask the format to describe itself — serde buffers the value before
//! it knows the type — which is the one thing a type-driven reify cannot do by
//! following a schema. They reach `Deserializer::deserialize_any`, and until
//! it was implemented every case in this file failed outright.
//!
//! [`representations_round_trip`] runs both oracles over the forms that work.
//! [`skip_drops_the_field`] pins the one that cannot: the limit is serde's,
//! not Spytial's.

use serde::{Deserialize, Serialize};
use spytial_eval_corpus::{parity, Bytes};

// ── plain renaming ───────────────────────────────────────────────────────

/// `rename` moves a field name; `Debug` still prints the Rust name.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct Renamed {
    #[serde(rename = "outer")]
    inner: i32,
}

/// `rename_all` moves every field name at once.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
struct RenamedAll {
    long_field_name: i32,
}

// ── flatten ──────────────────────────────────────────────────────────────

/// Flattened into its parent, which turns the parent into a `map`.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct Inner {
    y: i32,
}

/// `flatten` erases the struct boundary in the datum but not in `{:?}`.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct Flat {
    x: i32,
    #[serde(flatten)]
    rest: Inner,
}

/// An externally tagged enum *inside* a flattened parent. The enum's atom is
/// reached through `deserialize_any`, so this is the case that needs a variant
/// atom told apart from a struct atom.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
enum External {
    Unit,
    Newtype(i32),
    Tuple(i32, i32),
    Struct { w: i32 },
}

/// Pairs a flattened struct with each externally tagged variant shape.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct FlatWithEnum {
    #[serde(flatten)]
    rest: Inner,
    e: External,
}

// ── tagged enum representations ──────────────────────────────────────────

/// Internally tagged: the variant name becomes a field, so the whole value is
/// written as a struct.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(tag = "kind")]
enum Internally {
    Struct { v: i32 },
    Unit,
}

/// Adjacently tagged: tag and content sit side by side in one struct.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(tag = "t", content = "c")]
enum Adjacently {
    Newtype(i32),
    Struct { v: i32 },
    Unit,
}

/// Untagged: the variant is recovered by shape alone, which is what forces the
/// buffering.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(untagged)]
enum Untagged {
    Struct { q: i32 },
    Number(i32),
    Text(String),
    Nothing,
}

// ── optional and defaulted fields ────────────────────────────────────────

/// `skip_serializing_if` omits a field from the datum entirely; `default`
/// puts it back.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct Optional {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    a: Option<i32>,
    #[serde(default)]
    b: i32,
}

// ── field names that shadow export's built-in relations ──────────────────

/// A struct field named after a built-in relation. Records are keyed by source
/// type, so `idx` here is its own record (`FieldIdx.idx`), but it shares a
/// *name* with a sequence's positions, as `value` / `variant_value` do with
/// the relations a newtype struct and an enum variant use. Reaching these
/// through a self-describing
/// representation is what makes them interesting: `deserialize_any` has only
/// the atom to go on, so it must not read a field name as a payload marker.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct FieldIdx {
    idx: i32,
}

/// As [`FieldIdx`], for the map-entry relation.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct FieldMapEntry {
    map_entry: i32,
}

/// As [`FieldIdx`], for the newtype-struct payload relation.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct FieldValue {
    value: i32,
}

/// As [`FieldIdx`], for the enum-variant payload relation.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct FieldVariantValue {
    variant_value: i32,
}

/// Forces each shadowing struct through `deserialize_any`.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(untagged)]
enum Shadowing {
    Idx(FieldIdx),
    MapEntry(FieldMapEntry),
    Value(FieldValue),
    VariantValue(FieldVariantValue),
}

/// Externally tagged variants whose *field* names shadow the relations that
/// mark a variant's payload. A tuple variant's `idx` is ternary and a field
/// named `idx` is binary, so arity separates those. `variant_value` alone is
/// genuinely ambiguous with a newtype variant, and `TwoFields` is the case
/// that shows one more field is enough to resolve it.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
enum ExternalShadowed {
    Idx { idx: i32 },
    TwoFields { variant_value: i32, other: i32 },
}

/// Reaches [`ExternalShadowed`] through a flattened parent, so the variant's
/// atom is read by `deserialize_any` rather than by following the type.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct FlatWithShadowed {
    #[serde(flatten)]
    rest: Inner,
    e: ExternalShadowed,
}

/// One case: a label and the oracle run for it.
struct Case {
    what: &'static str,
    run: fn() -> Result<String, String>,
}

macro_rules! case {
    ($what:literal, $value:expr) => {
        Case {
            what: $what,
            run: || parity($value),
        }
    };
}

fn cases() -> Vec<Case> {
    vec![
        case!("rename", Renamed { inner: 5 }),
        case!("rename_all", RenamedAll { long_field_name: 5 }),
        case!(
            "flatten",
            Flat {
                x: 1,
                rest: Inner { y: 2 }
            }
        ),
        case!(
            "flatten + external unit",
            FlatWithEnum {
                rest: Inner { y: 2 },
                e: External::Unit
            }
        ),
        case!(
            "flatten + external newtype",
            FlatWithEnum {
                rest: Inner { y: 2 },
                e: External::Newtype(3)
            }
        ),
        case!(
            "flatten + external tuple",
            FlatWithEnum {
                rest: Inner { y: 2 },
                e: External::Tuple(3, 4)
            }
        ),
        case!(
            "flatten + external struct",
            FlatWithEnum {
                rest: Inner { y: 2 },
                e: External::Struct { w: 5 }
            }
        ),
        case!("internally tagged struct", Internally::Struct { v: 1 }),
        case!("internally tagged unit", Internally::Unit),
        case!("adjacently tagged newtype", Adjacently::Newtype(1)),
        case!("adjacently tagged struct", Adjacently::Struct { v: 1 }),
        case!("adjacently tagged unit", Adjacently::Unit),
        case!("untagged struct", Untagged::Struct { q: 1 }),
        case!("untagged number", Untagged::Number(7)),
        case!("untagged text", Untagged::Text("s".into())),
        case!("untagged unit", Untagged::Nothing),
        case!(
            "untagged struct { idx }",
            Shadowing::Idx(FieldIdx { idx: 9 })
        ),
        case!(
            "untagged struct { map_entry }",
            Shadowing::MapEntry(FieldMapEntry { map_entry: 9 })
        ),
        case!(
            "untagged struct { value }",
            Shadowing::Value(FieldValue { value: 9 })
        ),
        case!(
            "untagged struct { variant_value }",
            Shadowing::VariantValue(FieldVariantValue { variant_value: 9 })
        ),
        case!(
            "external variant { idx }",
            FlatWithShadowed {
                rest: Inner { y: 1 },
                e: ExternalShadowed::Idx { idx: 9 }
            }
        ),
        case!(
            "external variant { variant_value, .. }",
            FlatWithShadowed {
                rest: Inner { y: 1 },
                e: ExternalShadowed::TwoFields {
                    variant_value: 9,
                    other: 1
                }
            }
        ),
        case!(
            "untagged byte array",
            UntaggedBytes::Blob(Bytes(vec![1, 2, 3]))
        ),
        case!(
            "untagged byte array, empty",
            UntaggedBytes::Blob(Bytes(Vec::new()))
        ),
        case!(
            "flattened byte array",
            FlatWithBytes {
                rest: Inner { y: 1 },
                b: Bytes(vec![0, 255])
            }
        ),
        case!("skip_serializing_if, absent", Optional { a: None, b: 0 }),
        case!(
            "skip_serializing_if, present",
            Optional { a: Some(3), b: 9 }
        ),
    ]
}

// ── byte arrays under a self-describing representation ───────────────────

/// A byte array is the one scalar whose atom label is itself bracketed
/// (`[1, 2, 3]`). Every other case in this file is built from named or indexed
/// parts, so a byte array is the only way to reach `deserialize_any` with a
/// scalar that *looks* structural. A missing arm there hands the label over as
/// an enum variant name instead of bytes.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(untagged)]
enum UntaggedBytes {
    Blob(Bytes),
}

/// The same byte array, buffered by a flattened parent rather than an
/// untagged enum.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct FlatWithBytes {
    #[serde(flatten)]
    rest: Inner,
    b: Bytes,
}

/// Both oracles, over every representation attribute that can round-trip.
#[test]
fn representations_round_trip() {
    let cases = cases();
    let width = cases.iter().map(|c| c.what.len()).max().unwrap_or(0);
    let mut failures = Vec::new();

    println!("\nserde representation attributes\n");
    for case in &cases {
        match (case.run)() {
            Ok(printed) => println!("  = {:<width$}  {printed}", case.what),
            Err(why) => {
                println!("  ! {:<width$}  {why}", case.what);
                failures.push(format!("{}: {why}", case.what));
            }
        }
    }
    println!();

    assert!(
        failures.is_empty(),
        "{} of {} representations failed:\n  {}",
        failures.len(),
        cases.len(),
        failures.join("\n  ")
    );
}

/// `#[serde(skip)]` is the one representation neither oracle can satisfy, and
/// the reason is worth stating precisely: serde never shows the field to
/// anyone, so it is absent from the datum by construction. Spytial's
/// inspection mechanism for Rust *is* `Serialize`, so what `Serialize` hides
/// is outside what any amount of reify work could recover. `Debug` still
/// prints it, so R-inspect fails, and `Deserialize` fills it from `Default`.
///
/// Pinned rather than merely documented, so that a future change to either
/// side shows up here.
#[test]
fn skip_drops_the_field() {
    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct Skipped {
        a: i32,
        #[serde(skip)]
        b: i32,
    }

    let why = parity(Skipped { a: 1, b: 2 }).expect_err("#[serde(skip)] cannot round-trip");
    assert!(
        why.contains("Skipped { a: 1, b: 2 }") && why.contains("Skipped { a: 1, b: 0 }"),
        "expected the skipped field to come back as Default, got: {why}"
    );
}
