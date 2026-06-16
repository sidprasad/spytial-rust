//! Probe: does #[derive(SpytialDecorators)] handle pathological type shapes?
//!
//! These are intentionally minimal — the question is whether the macro EXPANDS
//! at all on each shape, not whether the result is semantically rich. If a
//! variant fails to compile, the macro has a hole worth fixing or documenting.

use serde::Serialize;
use spytial::SpytialDecorators;

#[derive(Serialize, SpytialDecorators)]
struct UnitStruct;

#[derive(Serialize, SpytialDecorators)]
struct EmptyNamed {}

#[derive(Serialize, SpytialDecorators)]
struct EmptyTuple();

#[derive(Serialize, SpytialDecorators)]
enum EmptyEnum {}

#[derive(Serialize, SpytialDecorators)]
#[allow(dead_code)]
enum AllUnit {
    A,
    B,
    C,
}

#[derive(Serialize, SpytialDecorators)]
enum Mixed {
    Unit,
    Tuple(u32, String),
    Named { name: String, age: u32 },
}

// Generic struct — does the type-walker even know what to do with T?
#[derive(Serialize, SpytialDecorators)]
struct Generic<T: Serialize> {
    value: T,
}

// Lifetime — does the macro choke on lifetimes?
#[derive(Serialize, SpytialDecorators)]
struct WithLifetime<'a> {
    s: &'a str,
}

// Newtype around a primitive
#[derive(Serialize, SpytialDecorators)]
struct NewtypeI32(i32);

// Newtype around a Vec
#[derive(Serialize, SpytialDecorators)]
struct NewtypeVec(Vec<i32>);

// A decorator attribute on each shape — does decorators() emit something useful?
#[derive(Serialize, SpytialDecorators)]
#[attribute(field = "key")]
struct DecoratedUnit;

#[derive(Serialize, SpytialDecorators)]
#[attribute(field = "key")]
enum DecoratedEmpty {}

use spytial::spytial_annotations::HasSpytialDecorators;

#[test]
fn unit_struct_has_empty_decorators() {
    let d = UnitStruct::decorators();
    assert!(d.constraints.is_empty());
    assert!(d.directives.is_empty());
}

#[test]
fn empty_named_struct_compiles() {
    let _ = EmptyNamed {};
    let _d = EmptyNamed::decorators();
}

#[test]
fn empty_tuple_struct_compiles() {
    let _ = EmptyTuple();
    let _d = EmptyTuple::decorators();
}

#[test]
fn zero_variant_enum_compiles() {
    // Can't construct a zero-variant enum, but the impl should exist.
    let _d = EmptyEnum::decorators();
}

#[test]
fn all_unit_enum_compiles() {
    let _ = AllUnit::A;
    let _d = AllUnit::decorators();
}

#[test]
fn mixed_enum_compiles() {
    let _ = Mixed::Unit;
    let _ = Mixed::Tuple(1, "x".into());
    let _ = Mixed::Named {
        name: "x".into(),
        age: 1,
    };
    let _d = Mixed::decorators();
}

#[test]
fn generic_struct_compiles() {
    let _ = Generic { value: 1u32 };
    let _d = Generic::<u32>::decorators();
}

#[test]
fn lifetime_struct_compiles() {
    let s = "hi";
    let _ = WithLifetime { s };
    let _d = WithLifetime::decorators();
}

#[test]
fn newtype_primitive_compiles() {
    let _ = NewtypeI32(7);
    let _d = NewtypeI32::decorators();
}

#[test]
fn newtype_vec_compiles() {
    let _ = NewtypeVec(vec![1, 2, 3]);
    let _d = NewtypeVec::decorators();
}

#[test]
fn decorated_unit_struct_emits_decorator() {
    let d = DecoratedUnit::decorators();
    // attribute should land somewhere — either as a constraint or a directive
    assert!(
        !d.constraints.is_empty() || !d.directives.is_empty(),
        "DecoratedUnit should have at least one decorator from #[attribute(field=\"key\")]"
    );
}

#[test]
fn decorated_empty_enum_emits_decorator() {
    let d = DecoratedEmpty::decorators();
    assert!(
        !d.constraints.is_empty() || !d.directives.is_empty(),
        "DecoratedEmpty should have at least one decorator"
    );
}
