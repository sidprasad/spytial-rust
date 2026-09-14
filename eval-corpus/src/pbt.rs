//! Randomized generation for the two reify oracles.
//!
//! The curated corpus in [`crate::cases`] pins one value per serde category
//! plus the extremes worth naming in a paper. It cannot search. This module
//! adds the search: [`Nest`] is a single recursive type whose variants cover
//! every category in [`crate::SERDE_DATA_MODEL`], and [`nest`] is a proptest
//! [`Strategy`] over it. A generated value nests categories inside each other
//! to arbitrary depth, which is the coverage no hand-written list provides.
//!
//! # Shape variety, not just value variety
//!
//! Nesting alone would exercise one struct shape over and over. Export's
//! subtler behaviour lives in *shape* rather than in values: relation records
//! are keyed by source type and name, so a field name shared with another
//! type or with a built-in like `idx` splits into records of one name, and an
//! enum's variants can still mix arities in one record. So the generator also
//! varies field counts (zero,
//! one, two, three), tuple arities (one, two, three), field names that shadow
//! every built-in relation ([`Shadowed`]), and one value carrying two
//! different types that share a field name ([`Nest::Collide`]).
//!
//! # Two scalar domains, because there are two kinds of bug
//!
//! [`Pool`] selects how wide the scalar generators draw.
//!
//! [`Pool::Wide`] is edge-biased: three times in four from a curated pool
//! (type bounds, both signed zeros, the infinities, quoting hazards), and
//! otherwise the full range. This hunts **parsing** bugs, where one value
//! fails to survive the trip through its atom label.
//!
//! [`Pool::Narrow`] gives every type two values. This hunts **interning**
//! bugs. Export gives equal primitives one shared atom, so an interning key
//! coarser than value identity only misbehaves when one datum holds two
//! scalars that collide under it. Wide generation essentially never poses that
//! question: a uniformly random `i64` does not repeat. Narrow generation poses
//! it constantly, and shrinks to counterexamples that read as source.
//!
//! # Floats and the two oracles
//!
//! [`Floats`] selects whether NaN may be generated, because the oracles have
//! complementary blind spots (see [`crate::Support`]). NaN is not `==` to
//! itself, so a datum containing one can only be judged by R-inspect. Maps are
//! keyed and ordered (`BTreeMap`), so `{:?}` stays deterministic and both
//! oracles apply.

use std::collections::{BTreeMap, BTreeSet};

use proptest::num as float_class;
use proptest::prelude::*;
use proptest::strategy::Union;
use serde::{Deserialize, Serialize};

use crate::{Bytes, Meters, Pair, UnitStruct};

/// Exercises `struct` with recursively nested fields, so the `struct` category
/// can appear at any depth rather than only wrapping primitives.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Node {
    /// Left child.
    pub left: Box<Nest>,
    /// Right child.
    pub right: Box<Nest>,
}

/// Exercises `struct` with no fields at all.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Rec0 {}

/// Exercises `struct` with exactly one field.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Rec1 {
    /// The only field.
    pub only: Box<Nest>,
}

/// Exercises `struct` with three fields, so relation fan-out from one atom is
/// not always two.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Rec3 {
    /// First field.
    pub p: Box<Nest>,
    /// Second field.
    pub q: Box<Nest>,
    /// Third field.
    pub r: Box<Nest>,
}

/// Every field named after one of export's built-in relations.
///
/// A field called `idx` is a record of the same *name* as a sequence's
/// positions (`Shadowed.idx` beside `sequence.idx`), and a field called
/// `value` or `variant_value` shares its name with the relations a newtype
/// struct and an enum variant emit. Reconstruction has to bucket by source
/// atom rather than trust the name.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Shadowed {
    /// Shares a name with a sequence's position relation.
    pub idx: Box<Nest>,
    /// Shares a name with a map's entry relation.
    pub map_entry: Box<Nest>,
    /// Shares a name with a newtype struct's payload relation.
    pub value: Box<Nest>,
    /// Shares a name with an enum variant's payload relation.
    pub variant_value: Box<Nest>,
}

/// One half of a same-field-name pair; see [`Nest::Collide`].
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct NameA {
    /// Same name as [`NameB::name`], different type.
    pub name: Box<Nest>,
}

/// The other half of the same-field-name pair; see [`Nest::Collide`].
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct NameB {
    /// Same name as [`NameA::name`], different type.
    pub name: Box<Nest>,
}

/// A recursive value covering every category in [`crate::SERDE_DATA_MODEL`].
///
/// Every payload-carrying variant is itself an enum variant, so a generated
/// value always exercises `newtype_variant`, `tuple_variant` or
/// `struct_variant` on top of whatever it wraps. [`Nest::categories`] reports
/// the full set, not just the outermost one.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum Nest {
    /// `bool`
    Bool(bool),
    /// `i8`
    I8(i8),
    /// `i16`
    I16(i16),
    /// `i32`
    I32(i32),
    /// `i64`
    I64(i64),
    /// `i128`
    I128(i128),
    /// `u8`
    U8(u8),
    /// `u16`
    U16(u16),
    /// `u32`
    U32(u32),
    /// `u64`
    U64(u64),
    /// `u128`
    U128(u128),
    /// `f32`
    F32(f32),
    /// `f64`
    F64(f64),
    /// `char`
    Char(char),
    /// `string`
    Str(String),
    /// `byte array`, via a newtype struct that forces `serialize_bytes`.
    Blob(Bytes),
    /// `unit` — the payload is `()`.
    Nil(()),
    /// `unit_struct`
    Unit(UnitStruct),
    /// `unit_variant` — no payload at all.
    Nothing,
    /// `newtype_struct`
    Newtype(Meters),
    /// `tuple_struct`
    TupleStruct(Pair),
    /// `option`
    Opt(Option<Box<Nest>>),
    /// `newtype_variant` carrying a nested value.
    Wrap(Box<Nest>),
    /// `seq`
    Seq(Vec<Nest>),
    /// `tuple` — a real Rust tuple, distinct from `tuple_variant`.
    Tup((Box<Nest>, Box<Nest>)),
    /// `tuple_variant`
    TupVariant(Box<Nest>, Box<Nest>),
    /// `map`
    Map(BTreeMap<String, Nest>),
    /// `struct`
    Struct(Node),
    /// `struct` with no fields.
    Fields0(Rec0),
    /// `struct` with one field.
    Fields1(Rec1),
    /// `struct` with three fields.
    Fields3(Rec3),
    /// `struct` whose every field name shadows a built-in relation.
    Shadow(Shadowed),
    /// Two different types carrying the same field name, in one value. They
    /// are separate records of one name (`NameA.x`, `NameB.x`), so this is
    /// the shape that exercises the split.
    Collide(NameA, NameB),
    /// `tuple` of one, so tuple arity is not always two.
    Tup1((Box<Nest>,)),
    /// `tuple` of three.
    Tup3((Box<Nest>, Box<Nest>, Box<Nest>)),
    /// `struct_variant`
    StructVariant {
        /// First field.
        a: Box<Nest>,
        /// Second field.
        b: Box<Nest>,
    },
}

impl Nest {
    /// Every serde category this value exercises, its own variant step
    /// included. Used by the coverage test to prove the generator can still
    /// reach all 29 categories, which randomization otherwise leaves to chance.
    pub fn categories(&self) -> BTreeSet<&'static str> {
        let mut out = BTreeSet::new();
        self.collect(&mut out);
        out
    }

    fn collect(&self, out: &mut BTreeSet<&'static str>) {
        // Every variant below is reached through one of serde's three
        // payload-carrying variant kinds; record that step before the payload.
        let own = match self {
            Nest::Nothing => "unit_variant",
            Nest::TupVariant(..) | Nest::Collide(..) => "tuple_variant",
            Nest::StructVariant { .. } => "struct_variant",
            _ => "newtype_variant",
        };
        out.insert(own);

        match self {
            Nest::Bool(_) => drop(out.insert("bool")),
            Nest::I8(_) => drop(out.insert("i8")),
            Nest::I16(_) => drop(out.insert("i16")),
            Nest::I32(_) => drop(out.insert("i32")),
            Nest::I64(_) => drop(out.insert("i64")),
            Nest::I128(_) => drop(out.insert("i128")),
            Nest::U8(_) => drop(out.insert("u8")),
            Nest::U16(_) => drop(out.insert("u16")),
            Nest::U32(_) => drop(out.insert("u32")),
            Nest::U64(_) => drop(out.insert("u64")),
            Nest::U128(_) => drop(out.insert("u128")),
            Nest::F32(_) => drop(out.insert("f32")),
            Nest::F64(_) => drop(out.insert("f64")),
            Nest::Char(_) => drop(out.insert("char")),
            Nest::Str(_) => drop(out.insert("string")),
            Nest::Blob(_) => {
                out.insert("newtype_struct");
                out.insert("byte array");
            }
            Nest::Nil(()) => drop(out.insert("unit")),
            Nest::Unit(_) => drop(out.insert("unit_struct")),
            Nest::Nothing => {}
            Nest::Newtype(_) => {
                out.insert("newtype_struct");
                out.insert("f64");
            }
            Nest::TupleStruct(_) => {
                out.insert("tuple_struct");
                out.insert("i32");
            }
            Nest::Opt(inner) => {
                out.insert("option");
                if let Some(v) = inner {
                    v.collect(out);
                }
            }
            Nest::Wrap(v) => v.collect(out),
            Nest::Seq(vs) => {
                out.insert("seq");
                for v in vs {
                    v.collect(out);
                }
            }
            Nest::Tup((a, b)) => {
                out.insert("tuple");
                a.collect(out);
                b.collect(out);
            }
            Nest::TupVariant(a, b) => {
                a.collect(out);
                b.collect(out);
            }
            Nest::Map(m) => {
                out.insert("map");
                out.insert("string");
                for v in m.values() {
                    v.collect(out);
                }
            }
            Nest::Struct(n) => {
                out.insert("struct");
                n.left.collect(out);
                n.right.collect(out);
            }
            Nest::Fields0(_) => drop(out.insert("struct")),
            Nest::Fields1(n) => {
                out.insert("struct");
                n.only.collect(out);
            }
            Nest::Fields3(n) => {
                out.insert("struct");
                n.p.collect(out);
                n.q.collect(out);
                n.r.collect(out);
            }
            Nest::Shadow(n) => {
                out.insert("struct");
                n.idx.collect(out);
                n.map_entry.collect(out);
                n.value.collect(out);
                n.variant_value.collect(out);
            }
            Nest::Collide(a, b) => {
                out.insert("struct");
                a.name.collect(out);
                b.name.collect(out);
            }
            Nest::Tup1((a,)) => {
                out.insert("tuple");
                a.collect(out);
            }
            Nest::Tup3((a, b, c)) => {
                out.insert("tuple");
                a.collect(out);
                b.collect(out);
                c.collect(out);
            }
            Nest::StructVariant { a, b } => {
                a.collect(out);
                b.collect(out);
            }
        }
    }
}

/// Which float classes [`nest`] may produce, and so which oracles apply.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Floats {
    /// No NaN, so every generated value is `==` to itself and both oracles hold.
    Comparable,
    /// NaN allowed. R-eq is meaningless; judge with [`crate::inspect_only`].
    WithNan,
}

/// How wide a domain the scalar generators draw from.
///
/// Export interns primitives by value, so two equal scalars become one atom.
/// Any place where the interning key is coarser than value identity is a
/// defect, but it is only *reachable* when one datum holds two scalars that
/// collide under that key. A uniformly random `i64` never repeats, so a wide
/// generator can run for a very long time without ever posing the question.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pool {
    /// Type bounds, both signed zeros, the infinities and quoting hazards,
    /// three times in four; the full range otherwise. Hunts value-parsing bugs.
    Wide,
    /// Two values per type, chosen so repeats are near-certain and the two are
    /// distinguishable by `{:?}` but easy to conflate. Hunts interning bugs.
    Narrow,
}

/// Define a scalar strategy for both [`Pool`] settings:
/// `pooled!(fn_name, type, [wide pool], [narrow pool])`.
macro_rules! pooled {
    ($name:ident, $t:ty, [$($w:expr),* $(,)?], [$($n:expr),* $(,)?]) => {
        fn $name(pool: Pool) -> BoxedStrategy<$t> {
            match pool {
                Pool::Narrow => prop::sample::select(vec![$($n),*]).boxed(),
                Pool::Wide => prop_oneof![
                    3 => prop::sample::select(vec![$($w),*]),
                    1 => any::<$t>(),
                ]
                .boxed(),
            }
        }
    };
}

pooled!(bools, bool, [true, false], [true, false]);
pooled!(i8s, i8, [0, 1, -1, i8::MIN, i8::MAX], [0, 1]);
pooled!(i16s, i16, [0, 1, -1, i16::MIN, i16::MAX], [0, 1]);
pooled!(i32s, i32, [0, 1, -1, i32::MIN, i32::MAX], [0, 1]);
pooled!(i64s, i64, [0, 1, -1, i64::MIN, i64::MAX], [0, 1]);
pooled!(i128s, i128, [0, 1, -1, i128::MIN, i128::MAX], [0, 1]);
pooled!(u8s, u8, [0, 1, u8::MAX], [0, 1]);
pooled!(u16s, u16, [0, 1, u16::MAX], [0, 1]);
pooled!(u32s, u32, [0, 1, u32::MAX], [0, 1]);
pooled!(u64s, u64, [0, 1, u64::MAX], [0, 1]);
pooled!(u128s, u128, [0, 1, u128::MAX], [0, 1]);
pooled!(
    chars,
    char,
    ['\0', '\n', '\t', '"', '\\', '\'', 'z', 'λ', '💡', '中'],
    ['a', 'A']
);

/// Both signed zeros are in every pool on purpose. Export folds `-0.0` onto
/// `0.0` when it computes an interning key, because Rust considers them equal;
/// `{:?}` does not. A datum holding both is where that shows.
fn f64s(floats: Floats, pool: Pool) -> BoxedStrategy<f64> {
    let mut narrow = vec![0.0_f64, -0.0];
    let mut wide = vec![
        0.0_f64,
        -0.0,
        1.0,
        -1.0,
        0.5,
        f64::MIN,
        f64::MAX,
        f64::MIN_POSITIVE,
        f64::INFINITY,
        f64::NEG_INFINITY,
    ];
    let full = match floats {
        Floats::Comparable => {
            float_class::f64::POSITIVE
                | float_class::f64::NEGATIVE
                | float_class::f64::NORMAL
                | float_class::f64::SUBNORMAL
                | float_class::f64::ZERO
                | float_class::f64::INFINITE
        }
        Floats::WithNan => {
            wide.push(f64::NAN);
            narrow.push(f64::NAN);
            float_class::f64::ANY
        }
    };
    match pool {
        Pool::Narrow => prop::sample::select(narrow).boxed(),
        Pool::Wide => prop_oneof![3 => prop::sample::select(wide), 1 => full].boxed(),
    }
}

/// [`f64s`], narrowed to `f32`.
fn f32s(floats: Floats, pool: Pool) -> BoxedStrategy<f32> {
    let mut narrow = vec![0.0_f32, -0.0];
    let mut wide = vec![
        0.0_f32,
        -0.0,
        1.0,
        -1.0,
        0.5,
        f32::MIN,
        f32::MAX,
        f32::MIN_POSITIVE,
        f32::INFINITY,
        f32::NEG_INFINITY,
    ];
    let full = match floats {
        Floats::Comparable => {
            float_class::f32::POSITIVE
                | float_class::f32::NEGATIVE
                | float_class::f32::NORMAL
                | float_class::f32::SUBNORMAL
                | float_class::f32::ZERO
                | float_class::f32::INFINITE
        }
        Floats::WithNan => {
            wide.push(f32::NAN);
            narrow.push(f32::NAN);
            float_class::f32::ANY
        }
    };
    match pool {
        Pool::Narrow => prop::sample::select(narrow).boxed(),
        Pool::Wide => prop_oneof![3 => prop::sample::select(wide), 1 => full].boxed(),
    }
}

/// Short strings over the pooled `char`s, so quotes, backslashes, newlines and
/// non-BMP scalars all reach the atom label. Escaping is `Debug`'s job, not
/// export's, so R-inspect is what checks the label survived raw.
fn text(pool: Pool) -> BoxedStrategy<String> {
    let len = match pool {
        Pool::Narrow => 0..2,
        Pool::Wide => 0..4,
    };
    prop::collection::vec(chars(pool), len)
        .prop_map(|cs| cs.into_iter().collect())
        .boxed()
}

/// A strategy over [`Nest`]: every serde category, nested to arbitrary depth.
///
/// `floats` decides whether NaN may appear, and so which oracle can judge the
/// result ([`Floats`]). `pool` decides how wide the scalar domain is, which
/// decides whether interning collisions are reachable at all ([`Pool`]).
pub fn nest(floats: Floats, pool: Pool) -> BoxedStrategy<Nest> {
    let leaf = Union::new(vec![
        bools(pool).prop_map(Nest::Bool).boxed(),
        i8s(pool).prop_map(Nest::I8).boxed(),
        i16s(pool).prop_map(Nest::I16).boxed(),
        i32s(pool).prop_map(Nest::I32).boxed(),
        i64s(pool).prop_map(Nest::I64).boxed(),
        i128s(pool).prop_map(Nest::I128).boxed(),
        u8s(pool).prop_map(Nest::U8).boxed(),
        u16s(pool).prop_map(Nest::U16).boxed(),
        u32s(pool).prop_map(Nest::U32).boxed(),
        u64s(pool).prop_map(Nest::U64).boxed(),
        u128s(pool).prop_map(Nest::U128).boxed(),
        f32s(floats, pool).prop_map(Nest::F32).boxed(),
        f64s(floats, pool).prop_map(Nest::F64).boxed(),
        chars(pool).prop_map(Nest::Char).boxed(),
        text(pool).prop_map(Nest::Str).boxed(),
        prop::collection::vec(u8s(pool), 0..4)
            .prop_map(|b| Nest::Blob(Bytes(b)))
            .boxed(),
        Just(Nest::Nil(())).boxed(),
        Just(Nest::Fields0(Rec0 {})).boxed(),
        Just(Nest::Unit(UnitStruct)).boxed(),
        Just(Nest::Nothing).boxed(),
        f64s(floats, pool)
            .prop_map(|f| Nest::Newtype(Meters(f)))
            .boxed(),
        (i32s(pool), i32s(pool))
            .prop_map(|(a, b)| Nest::TupleStruct(Pair(a, b)))
            .boxed(),
    ]);

    // depth 4, ~64 nodes, ~4 branches: deep enough to nest a category inside
    // three others, small enough that a shrunk counterexample is readable.
    // `Union` rather than `prop_oneof!`, which tops out at ten branches.
    leaf.prop_recursive(4, 64, 4, move |inner| {
        Union::new(vec![
            prop::option::of(inner.clone())
                .prop_map(|o| Nest::Opt(o.map(Box::new)))
                .boxed(),
            inner.clone().prop_map(|v| Nest::Wrap(Box::new(v))).boxed(),
            prop::collection::vec(inner.clone(), 0..4)
                .prop_map(Nest::Seq)
                .boxed(),
            (inner.clone(), inner.clone())
                .prop_map(|(a, b)| Nest::Tup((Box::new(a), Box::new(b))))
                .boxed(),
            (inner.clone(), inner.clone())
                .prop_map(|(a, b)| Nest::TupVariant(Box::new(a), Box::new(b)))
                .boxed(),
            prop::collection::btree_map(text(pool), inner.clone(), 0..4)
                .prop_map(Nest::Map)
                .boxed(),
            (inner.clone(), inner.clone())
                .prop_map(|(a, b)| {
                    Nest::Struct(Node {
                        left: Box::new(a),
                        right: Box::new(b),
                    })
                })
                .boxed(),
            (inner.clone(), inner.clone())
                .prop_map(|(a, b)| Nest::StructVariant {
                    a: Box::new(a),
                    b: Box::new(b),
                })
                .boxed(),
            // Shape variety, which value-space generation alone does not reach:
            // field counts other than two, tuple arities other than two, field
            // names that shadow built-in relations, and two distinct types
            // sharing one field name inside a single value.
            inner
                .clone()
                .prop_map(|v| Nest::Fields1(Rec1 { only: Box::new(v) }))
                .boxed(),
            (inner.clone(), inner.clone(), inner.clone())
                .prop_map(|(p, q, r)| {
                    Nest::Fields3(Rec3 {
                        p: Box::new(p),
                        q: Box::new(q),
                        r: Box::new(r),
                    })
                })
                .boxed(),
            (inner.clone(), inner.clone(), inner.clone(), inner.clone())
                .prop_map(|(i, m, v, vv)| {
                    Nest::Shadow(Shadowed {
                        idx: Box::new(i),
                        map_entry: Box::new(m),
                        value: Box::new(v),
                        variant_value: Box::new(vv),
                    })
                })
                .boxed(),
            (inner.clone(), inner.clone())
                .prop_map(|(a, b)| {
                    Nest::Collide(NameA { name: Box::new(a) }, NameB { name: Box::new(b) })
                })
                .boxed(),
            inner
                .clone()
                .prop_map(|a| Nest::Tup1((Box::new(a),)))
                .boxed(),
            (inner.clone(), inner.clone(), inner)
                .prop_map(|(a, b, c)| Nest::Tup3((Box::new(a), Box::new(b), Box::new(c))))
                .boxed(),
        ])
    })
    .boxed()
}
