//! Round-trip tests for `spytial::reify` — the inverse of `export`.
//!
//! Two oracles per case:
//!   * **R-eq**       `v == from_datum(export(v))`               (exact value)
//!   * **R-inspect**  `format!("{:?}", v) == replit(export(v))`  (Debug parity)
//!
//! Scope is everything-but-pointer-cycles: primitives, Option, sequences,
//! tuples, structs (named/tuple/newtype/unit), enums (all variant shapes),
//! maps, and arena/index "graphs" (cycles encoded as `usize` indices).

use std::collections::HashMap;
use std::fmt::Debug;

use serde::{Deserialize, Serialize};
use spytial::{export_json_instance, from_datum, replit};

/// R-eq only (use for types whose `{:?}` is order-sensitive, e.g. HashMap).
fn eq_roundtrip<T>(v: T)
where
    T: Serialize + serde::de::DeserializeOwned + Debug + PartialEq,
{
    let di = export_json_instance(&v);
    let back: T = from_datum(&di).unwrap_or_else(|e| panic!("from_datum failed: {e}"));
    assert_eq!(v, back, "R-eq round-trip mismatch");
}

/// R-eq + R-inspect.
fn full_roundtrip<T>(v: T)
where
    T: Serialize + serde::de::DeserializeOwned + Debug + PartialEq,
{
    let di = export_json_instance(&v);
    let back: T = from_datum(&di).unwrap_or_else(|e| panic!("from_datum failed: {e}"));
    assert_eq!(v, back, "R-eq round-trip mismatch");
    let printed = replit::<T>(&di).unwrap_or_else(|e| panic!("replit failed: {e}"));
    assert_eq!(
        format!("{:?}", v),
        printed,
        "R-inspect ({{:?}}) parity mismatch"
    );
}

#[test]
fn primitives() {
    full_roundtrip(42_i32);
    full_roundtrip(-7_i64);
    full_roundtrip(255_u8);
    full_roundtrip(true);
    full_roundtrip(false);
    full_roundtrip('z');
    full_roundtrip("hello".to_string());
    full_roundtrip(String::new());
}

#[test]
fn floats_canonicalize() {
    // Even whole-number floats: label is "3", parse -> 3.0_f64, Debug -> "3.0",
    // which matches the original's Debug.
    full_roundtrip(3.0_f64);
    full_roundtrip(3.5_f64);
    full_roundtrip(-0.25_f32);
    full_roundtrip(f64::INFINITY);
}

#[test]
fn options() {
    full_roundtrip(Some(5_i32));
    full_roundtrip(Option::<i32>::None);
    full_roundtrip(Some("x".to_string()));
}

#[test]
fn sequences_and_tuples() {
    full_roundtrip(vec![1_i32, 2, 3]);
    full_roundtrip(Vec::<i32>::new());
    full_roundtrip((1_i32, "x".to_string(), true));
    full_roundtrip(vec![vec![1_i32], vec![], vec![2, 3]]);
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct Point {
    x: i32,
    y: i32,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct Line {
    start: Point,
    end: Point,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct Pair(i32, i32);

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct Meters(f64);

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct Unit;

#[test]
fn structs() {
    full_roundtrip(Point { x: 1, y: 2 });
    full_roundtrip(Line {
        start: Point { x: 1, y: 2 },
        end: Point { x: 3, y: 4 },
    });
    full_roundtrip(Pair(3, 4));
    full_roundtrip(Meters(2.5));
    full_roundtrip(Unit);
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
enum Shape {
    Empty,
    Radius(f64),
    Offset(i32, i32),
    Rect { w: i32, h: i32 },
}

#[test]
fn enums_all_variant_shapes() {
    full_roundtrip(Shape::Empty);
    full_roundtrip(Shape::Radius(1.5));
    full_roundtrip(Shape::Offset(2, 3));
    full_roundtrip(Shape::Rect { w: 10, h: 5 });
    full_roundtrip(vec![
        Shape::Empty,
        Shape::Radius(2.0),
        Shape::Rect { w: 1, h: 1 },
    ]);
}

#[test]
fn maps_eq_only() {
    // {:?} for HashMap is iteration-order-dependent, so only R-eq applies.
    let mut m = HashMap::new();
    m.insert("a".to_string(), 1_i32);
    m.insert("b".to_string(), 2);
    m.insert("c".to_string(), 3);
    eq_roundtrip(m);
}

// --- The headline: cyclic *graphs* via arena/index encoding ---
//
// A graph cycle is expressed as a `usize` index, so the *data* is acyclic and
// round-trips through plain serde with full {:?} parity. No pointer cycles.

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct GraphNode {
    val: i32,
    next: Option<usize>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct Graph {
    nodes: Vec<GraphNode>,
    root: usize,
}

#[test]
fn arena_graph_with_index_cycles() {
    // node 0 -> node 1 -> node 0  (a 2-cycle, encoded as indices)
    let g = Graph {
        nodes: vec![
            GraphNode {
                val: 1,
                next: Some(1),
            },
            GraphNode {
                val: 2,
                next: Some(0),
            },
        ],
        root: 0,
    };
    full_roundtrip(g);

    // a self-loop: node 0 -> node 0
    let g2 = Graph {
        nodes: vec![GraphNode {
            val: 42,
            next: Some(0),
        }],
        root: 0,
    };
    full_roundtrip(g2);
}

#[test]
fn explicit_root() {
    // from_datum_root lets callers start from a chosen atom id; atom0 is the root.
    let v = Point { x: 7, y: 8 };
    let di = export_json_instance(&v);
    let back: Point = spytial::from_datum_root(&di, "atom0").unwrap();
    assert_eq!(v, back);
}

#[test]
fn nested_options_round_trip() {
    // `export` inserts a `Some` wrapper only around an inner `None`/`Some`, so
    // `Some(None)` is distinct from `None` and arbitrary nesting is recoverable,
    // while `Some(non-option)` stays unwrapped. (Was the Codex #67 collapse;
    // fixed for #68.)
    full_roundtrip(Some(Option::<i32>::None)); // Some(None) — was the bug
    full_roundtrip(Option::<i32>::None); // None
    full_roundtrip(Some(Some(5_i32))); // unwrapped, clean
    full_roundtrip(Some(Some(Option::<i32>::None))); // Some(Some(None))
    full_roundtrip(vec![Some(Some(1_i32)), Some(None), None]);
}

#[test]
fn colliding_relation_names_round_trip() {
    // Relations are keyed by source type and name, so same-named edges from
    // different types are separate records of one name (`Person.name`,
    // `Company.name`; issue #79, split since spytial-core 6.0). Reify buckets
    // tuples by source atom and relation name, so round-trips must be
    // unaffected — including a field named like a built-in relation of
    // different arity (`idx`), which now gets its own record beside
    // `sequence.idx`, and an enum whose variants share the enum as source
    // type, which is the one place a record still mixes arities.

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct Person {
        name: String,
    }
    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct Company {
        name: String,
    }
    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct BothNames {
        p: Person,
        c: Company,
    }
    full_roundtrip(BothNames {
        p: Person { name: "Ada".into() },
        c: Company {
            name: "Acme".into(),
        },
    });

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct HasIdx {
        idx: u32,
    }
    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct MixedArity {
        a: HasIdx,
        b: Vec<u32>,
    }
    full_roundtrip(MixedArity {
        a: HasIdx { idx: 9 },
        b: vec![10, 11],
    });

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct Meters(f64);
    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct HasValue {
        value: u32,
    }
    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct ValueCollision {
        a: HasValue,
        b: Meters,
    }
    full_roundtrip(ValueCollision {
        a: HasValue { value: 7 },
        b: Meters(1.5),
    });

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    enum Payload {
        Positional(u32, u32),
        Named { idx: u32 },
    }
    full_roundtrip(vec![Payload::Named { idx: 9 }, Payload::Positional(10, 11)]);
}

#[test]
fn self_describing_representations_round_trip() {
    // `flatten`, `untagged` and the tagged enum forms buffer a value before
    // they know its type, so they reach `deserialize_any` instead of following
    // `T`. They used to fail outright. The externally tagged enum below is the
    // demanding one: its atom is reached through the flattened parent, so it
    // has to be told apart from a plain struct atom by nothing but its label.

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct Inner {
        y: i32,
    }

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    enum External {
        Unit,
        Newtype(i32),
        Tuple(i32, i32),
        Struct { w: i32 },
    }

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct Flat {
        x: i32,
        #[serde(flatten)]
        rest: Inner,
        e: External,
    }

    for e in [
        External::Unit,
        External::Newtype(3),
        External::Tuple(3, 4),
        External::Struct { w: 5 },
    ] {
        full_roundtrip(Flat {
            x: 1,
            rest: Inner { y: 2 },
            e,
        });
    }

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    #[serde(tag = "kind")]
    enum Internally {
        Struct { v: i32 },
        Unit,
    }
    full_roundtrip(Internally::Struct { v: 1 });
    full_roundtrip(Internally::Unit);

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    #[serde(tag = "t", content = "c")]
    enum Adjacently {
        Newtype(i32),
        Unit,
    }
    full_roundtrip(Adjacently::Newtype(1));
    full_roundtrip(Adjacently::Unit);

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    #[serde(untagged)]
    enum Untagged {
        Struct { q: i32 },
        Number(i32),
        Text(String),
        Nothing,
    }
    full_roundtrip(Untagged::Struct { q: 1 });
    full_roundtrip(Untagged::Number(7));
    full_roundtrip(Untagged::Text("s".into()));
    full_roundtrip(Untagged::Nothing);
}
