//! Integration tests for the JSON data instance export.
//!
//! These tests verify the relational output format sent to spytial-core:
//! atoms, relations, type information, and how nested / annotated structs
//! compose.

use serde::Serialize;
use spytial::export::export_json_instance;
use spytial::jsondata::{IAtom, IRelation, JsonDataInstance};
use spytial::spytial_annotations::{to_yaml, Constraint, Directive, HasSpytialDecorators};
use spytial::SpytialDecorators;

// ──────────────────────────────────────────────
// Helpers
// ──────────────────────────────────────────────

/// Every record of a relation name, in datum order. A name has one record per
/// source type, so this is more than one exactly when types share a name.
fn relations_named<'a>(instance: &'a JsonDataInstance, name: &str) -> Vec<&'a IRelation> {
    instance
        .relations
        .iter()
        .filter(|r| r.name == name)
        .collect()
}

/// Find the single record of a relation name, panicking if absent — or if the
/// name is split across several records, which must be looked up by id.
fn relation<'a>(instance: &'a JsonDataInstance, name: &str) -> &'a IRelation {
    match relations_named(instance, name).as_slice() {
        [one] => one,
        [] => {
            let names: Vec<&str> = instance.relations.iter().map(|r| r.name.as_str()).collect();
            panic!("no relation named {:?}; available: {:?}", name, names)
        }
        many => {
            let ids: Vec<&str> = many.iter().map(|r| r.id.as_str()).collect();
            panic!(
                "relation {:?} has {} records {:?}; look one up by id",
                name,
                many.len(),
                ids
            )
        }
    }
}

/// Find a relation record by id (`"{source type}.{name}"`), panicking if absent.
fn relation_by_id<'a>(instance: &'a JsonDataInstance, id: &str) -> &'a IRelation {
    instance
        .relations
        .iter()
        .find(|r| r.id == id)
        .unwrap_or_else(|| {
            let ids: Vec<&str> = instance.relations.iter().map(|r| r.id.as_str()).collect();
            panic!("no relation with id {:?}; available: {:?}", id, ids)
        })
}

/// Find an atom by ID.
fn atom_by_id<'a>(instance: &'a JsonDataInstance, id: &str) -> &'a IAtom {
    instance
        .atoms
        .iter()
        .find(|a| a.id == id)
        .unwrap_or_else(|| panic!("no atom with id {:?}", id))
}

/// Find the first atom with a given type.
fn atom_by_type<'a>(instance: &'a JsonDataInstance, ty: &str) -> &'a IAtom {
    instance
        .atoms
        .iter()
        .find(|a| a.r#type == ty)
        .unwrap_or_else(|| panic!("no atom with type {:?}", ty))
}

/// Return all atoms whose type matches.
fn atoms_by_type<'a>(instance: &'a JsonDataInstance, ty: &str) -> Vec<&'a IAtom> {
    instance.atoms.iter().filter(|a| a.r#type == ty).collect()
}

// ──────────────────────────────────────────────
// 1. Flat struct → field-named relations
// ──────────────────────────────────────────────

#[derive(Serialize)]
struct Flat {
    name: String,
    age: u32,
}

#[test]
fn flat_struct_produces_field_relations() {
    let val = Flat {
        name: "Elizabeth".into(),
        age: 30,
    };
    let inst = export_json_instance(&val);

    // One atom for the struct itself
    let root = atom_by_type(&inst, "Flat");
    assert_eq!(root.label, "Flat");

    // name relation links root → string atom
    let name_rel = relation(&inst, "name");
    assert_eq!(name_rel.tuples.len(), 1);
    let name_target_id = &name_rel.tuples[0].atoms[1];
    let name_atom = atom_by_id(&inst, name_target_id);
    assert_eq!(name_atom.label, "Elizabeth");

    // age relation links root → u32 atom
    let age_rel = relation(&inst, "age");
    assert_eq!(age_rel.tuples.len(), 1);
    let age_target_id = &age_rel.tuples[0].atoms[1];
    let age_atom = atom_by_id(&inst, age_target_id);
    assert_eq!(age_atom.label, "30");
}

// ──────────────────────────────────────────────
// 2. Nested structs preserve type hierarchy
// ──────────────────────────────────────────────

#[derive(Serialize)]
struct Outer {
    child: Inner,
}

#[derive(Serialize)]
struct Inner {
    value: u32,
}

#[test]
fn nested_struct_creates_typed_atoms_and_relations() {
    let val = Outer {
        child: Inner { value: 42 },
    };
    let inst = export_json_instance(&val);

    // Both struct types appear as atom types
    assert!(inst.atoms.iter().any(|a| a.r#type == "Outer"));
    assert!(inst.atoms.iter().any(|a| a.r#type == "Inner"));

    // "child" relation connects Outer → Inner
    let child_rel = relation(&inst, "child");
    assert_eq!(child_rel.tuples.len(), 1);
    let inner_id = &child_rel.tuples[0].atoms[1];
    let inner_atom = atom_by_id(&inst, inner_id);
    assert_eq!(inner_atom.r#type, "Inner");

    // "value" relation connects Inner → u32
    let value_rel = relation(&inst, "value");
    let target_id = &value_rel.tuples[0].atoms[1];
    let target = atom_by_id(&inst, target_id);
    assert_eq!(target.label, "42");
}

// ──────────────────────────────────────────────
// 3. Vec produces idx relations
// ──────────────────────────────────────────────

#[derive(Serialize)]
struct HasVec {
    items: Vec<u32>,
}

#[test]
fn vec_field_produces_idx_relations() {
    let val = HasVec {
        items: vec![10, 20, 30],
    };
    let inst = export_json_instance(&val);

    let idx_rel = relation(&inst, "idx");
    assert_eq!(idx_rel.tuples.len(), 3, "one idx tuple per element");

    // Tuple positions refer to ordinary u64 atoms because that is how Serde
    // exposes usize. Their labels retain the Rust index values.
    let indices: Vec<&str> = idx_rel
        .tuples
        .iter()
        .map(|t| atom_by_id(&inst, &t.atoms[1]).label.as_str())
        .collect();
    assert!(indices.contains(&"0"));
    assert!(indices.contains(&"1"));
    assert!(indices.contains(&"2"));
    assert!(idx_rel
        .tuples
        .iter()
        .all(|t| atom_by_id(&inst, &t.atoms[1]).r#type == "u64"));
}

#[derive(Serialize)]
struct Positional(u8, u8);

#[derive(Serialize)]
enum PositionalVariant {
    Values(u8, u8),
}

#[derive(Serialize)]
struct AllPositionalShapes {
    ordinary_zero: u64,
    sequence: Vec<u8>,
    tuple: (u8, u8),
    tuple_struct: Positional,
    tuple_variant: PositionalVariant,
}

#[test]
fn every_idx_position_is_a_declared_serde_value() {
    let inst = export_json_instance(&AllPositionalShapes {
        ordinary_zero: 0,
        sequence: vec![10, 11],
        tuple: (20, 21),
        tuple_struct: Positional(30, 31),
        tuple_variant: PositionalVariant::Values(40, 41),
    });

    // One `idx` record per positional emitter, since each has its own source
    // type; the position rule below holds across all of them.
    let idx = relations_named(&inst, "idx");
    let mut ids: Vec<&str> = idx.iter().map(|r| r.id.as_str()).collect();
    ids.sort();
    assert_eq!(
        ids,
        vec![
            "PositionalVariant.idx",
            "sequence.idx",
            "tuple.idx",
            "tuple_struct.idx"
        ]
    );
    let idx_tuples: Vec<_> = idx.iter().flat_map(|r| r.tuples.iter()).collect();
    assert_eq!(idx_tuples.len(), 8, "all four positional emitters ran");

    let declared: std::collections::HashSet<&str> =
        inst.atoms.iter().map(|atom| atom.id.as_str()).collect();
    for tuple in &idx_tuples {
        assert!(tuple.atoms.iter().all(|id| declared.contains(id.as_str())));
        assert_eq!(atom_by_id(&inst, &tuple.atoms[1]).r#type, "u64");
    }

    let positions = atoms_by_type(&inst, "u64");
    assert_eq!(positions.len(), 2, "equal positions share value atoms");
    assert!(positions.iter().any(|atom| atom.label == "0"));
    assert!(positions.iter().any(|atom| atom.label == "1"));
    assert_eq!(
        relation(&inst, "ordinary_zero").tuples[0].atoms[1],
        idx_tuples
            .iter()
            .map(|tuple| &tuple.atoms[1])
            .find(|id| atom_by_id(&inst, id).label == "0")
            .expect("idx should contain position zero")
            .as_str(),
        "an index and an ordinary u64 share the value Serde exposes",
    );
}

// ──────────────────────────────────────────────
// 4. Option<T> / Box<T> unwrapping
// ──────────────────────────────────────────────

#[derive(Serialize)]
struct WithOption {
    maybe: Option<u32>,
}

#[test]
fn option_some_unwraps_to_inner_value() {
    let val = WithOption { maybe: Some(99) };
    let inst = export_json_instance(&val);

    let maybe_rel = relation(&inst, "maybe");
    assert_eq!(maybe_rel.tuples.len(), 1);
    let target_id = &maybe_rel.tuples[0].atoms[1];
    let target = atom_by_id(&inst, target_id);
    assert_eq!(target.label, "99");
}

#[test]
fn option_none_produces_none_atom() {
    let val = WithOption { maybe: None };
    let inst = export_json_instance(&val);

    let maybe_rel = relation(&inst, "maybe");
    assert_eq!(maybe_rel.tuples.len(), 1);
    let target_id = &maybe_rel.tuples[0].atoms[1];
    let target = atom_by_id(&inst, target_id);
    assert_eq!(target.r#type, "None");
}

#[derive(Serialize)]
struct NestedOption {
    v: Option<Option<u32>>,
}

#[test]
fn some_wraps_only_around_inner_none() {
    // Some(None) gets a `Some` wrapper atom pointing at the `None`, so it is
    // distinguishable from a plain `None`. (Some of a non-option still unwraps,
    // covered by option_some_unwraps_to_inner_value.)
    let inst = export_json_instance(&NestedOption { v: Some(None) });
    let target = atom_by_id(&inst, &relation(&inst, "v").tuples[0].atoms[1]);
    assert_eq!(target.r#type, "Some");
    let inner = atom_by_id(&inst, &relation(&inst, "value").tuples[0].atoms[1]);
    assert_eq!(inner.r#type, "None");

    // Plain None stays a bare None atom (no wrapper).
    let inst_none = export_json_instance(&NestedOption { v: None });
    let ntarget = atom_by_id(&inst_none, &relation(&inst_none, "v").tuples[0].atoms[1]);
    assert_eq!(ntarget.r#type, "None");
}

// ──────────────────────────────────────────────
// 5. Enum variants
// ──────────────────────────────────────────────

#[derive(Serialize)]
#[allow(dead_code)]
enum Status {
    Active,
    Inactive,
}

#[derive(Serialize)]
struct WithEnum {
    status: Status,
}

#[test]
fn unit_enum_variant_becomes_typed_atom() {
    let val = WithEnum {
        status: Status::Active,
    };
    let inst = export_json_instance(&val);

    // The variant produces an atom with the enum type name and variant label
    let status_rel = relation(&inst, "status");
    let target_id = &status_rel.tuples[0].atoms[1];
    let target = atom_by_id(&inst, target_id);
    assert_eq!(target.r#type, "Status");
    assert_eq!(target.label, "Active");
}

// ──────────────────────────────────────────────
// 6. Equal None values share an atom
// ──────────────────────────────────────────────

#[derive(Serialize)]
struct TwoOptions {
    a: Option<u32>,
    b: Option<u32>,
}

#[test]
fn none_atoms_are_deduplicated() {
    let val = TwoOptions { a: None, b: None };
    let inst = export_json_instance(&val);

    let none_atoms = atoms_by_type(&inst, "None");
    assert_eq!(
        none_atoms.len(),
        1,
        "both None fields should share a single atom"
    );
}

// ──────────────────────────────────────────────
// 7. Equal primitive values share atoms
// ──────────────────────────────────────────────

#[derive(Serialize)]
struct Flags {
    a: bool,
    b: bool,
}

#[test]
fn equal_boolean_values_share_an_atom() {
    let val = Flags { a: true, b: true };
    let inst = export_json_instance(&val);

    let true_atoms: Vec<_> = inst
        .atoms
        .iter()
        .filter(|a| a.r#type == "bool" && a.label == "true")
        .collect();
    assert_eq!(
        true_atoms.len(),
        1,
        "both `true` values should share one atom"
    );
}

#[derive(Serialize)]
struct RepeatedValues {
    first_number: u32,
    second_number: u32,
    first_string: String,
    second_string: String,
    positive_zero: f64,
    negative_zero: f64,
    first_nan: f64,
    second_nan: f64,
}

#[test]
fn primitive_atoms_follow_rust_value_equality() {
    let inst = export_json_instance(&RepeatedValues {
        first_number: 7,
        second_number: 7,
        first_string: "same".into(),
        second_string: "same".into(),
        positive_zero: 0.0,
        negative_zero: -0.0,
        first_nan: f64::NAN,
        second_nan: f64::NAN,
    });

    assert_eq!(
        relation(&inst, "first_number").tuples[0].atoms[1],
        relation(&inst, "second_number").tuples[0].atoms[1],
    );
    assert_eq!(
        relation(&inst, "first_string").tuples[0].atoms[1],
        relation(&inst, "second_string").tuples[0].atoms[1],
    );
    assert_ne!(
        relation(&inst, "positive_zero").tuples[0].atoms[1],
        relation(&inst, "negative_zero").tuples[0].atoms[1],
        "`==` calls the signed zeros equal but `{{:?}}` prints them differently, \
         so they are keyed by bit pattern and kept apart",
    );
    assert_ne!(
        relation(&inst, "first_nan").tuples[0].atoms[1],
        relation(&inst, "second_nan").tuples[0].atoms[1],
        "Rust considers NaN unequal even to itself",
    );
}

// ──────────────────────────────────────────────
// 8. Recursive types (tree-like)
// ──────────────────────────────────────────────

#[derive(Serialize)]
struct TreeNode {
    val: u32,
    left: Option<Box<TreeNode>>,
    right: Option<Box<TreeNode>>,
}

#[test]
fn recursive_struct_produces_multiple_typed_atoms() {
    let tree = TreeNode {
        val: 1,
        left: Some(Box::new(TreeNode {
            val: 2,
            left: None,
            right: None,
        })),
        right: Some(Box::new(TreeNode {
            val: 3,
            left: None,
            right: None,
        })),
    };
    let inst = export_json_instance(&tree);

    let tree_atoms = atoms_by_type(&inst, "TreeNode");
    assert_eq!(tree_atoms.len(), 3, "three TreeNode instances");

    // "val" relation should have three tuples (one per node)
    let val_rel = relation(&inst, "val");
    assert_eq!(val_rel.tuples.len(), 3);

    // "left" and "right" relations should exist
    let left_rel = relation(&inst, "left");
    let right_rel = relation(&inst, "right");
    assert!(!left_rel.tuples.is_empty());
    assert!(!right_rel.tuples.is_empty());
}

// ──────────────────────────────────────────────
// 9. Decorators are inherited through nested types
// ──────────────────────────────────────────────

// These two use the deprecated `atom_color` on purpose: what they check is that
// a nested type's decorators reach the parent's spec, and the legacy form is the
// shortest thing that produces one distinguishable directive per type.
#[derive(Serialize, SpytialDecorators)]
#[allow(deprecated)]
#[atom_color(selector = "{x : Parent | true}", value = "blue")]
struct Parent {
    child: Child,
}

#[derive(Serialize, SpytialDecorators)]
#[allow(deprecated)]
#[atom_color(selector = "{x : Child | true}", value = "red")]
#[attribute(field = "name")]
struct Child {
    name: String,
}

#[test]
fn parent_decorators_include_child_decorators() {
    let parent_decs = Parent::decorators();

    // Parent should have its own atom_color AND Child's atom_color + attribute
    let atom_colors: Vec<_> = parent_decs
        .directives
        .iter()
        .filter(|d| matches!(d, Directive::AtomStyle(_)))
        .collect();
    assert_eq!(
        atom_colors.len(),
        2,
        "parent should include both its own and child's atom_color"
    );

    let attributes: Vec<_> = parent_decs
        .directives
        .iter()
        .filter(|d| matches!(d, Directive::Attribute(_)))
        .collect();
    assert_eq!(
        attributes.len(),
        1,
        "child's #[attribute] should be inherited by parent"
    );
}

#[test]
fn child_decorators_are_independent() {
    let child_decs = Child::decorators();

    // Child should only have its own decorators, not parent's
    let atom_colors: Vec<_> = child_decs
        .directives
        .iter()
        .filter(|d| matches!(d, Directive::AtomStyle(_)))
        .collect();
    assert_eq!(atom_colors.len(), 1, "child has only its own atom_color");
}

// ──────────────────────────────────────────────
// 10. Decorators inherited through Vec<T>
// ──────────────────────────────────────────────

#[derive(Serialize, SpytialDecorators)]
struct Team {
    members: Vec<Member>,
}

#[derive(Serialize, SpytialDecorators)]
#[attribute(field = "role")]
#[hide_field(field = "highlighted")]
struct Member {
    role: String,
}

#[test]
fn decorators_inherited_through_vec() {
    let team_decs = Team::decorators();

    // Team has no own decorators, but should include Member's via Vec<Member>
    assert!(
        team_decs.directives.iter().any(|d| matches!(
            d,
            Directive::Attribute(a) if a.attribute.field == "role"
        )),
        "Member's #[attribute] should be inherited through Vec<Member>"
    );
    assert!(
        team_decs.directives.iter().any(|d| matches!(
            d,
            Directive::HideField(h) if h.hide_field.field == "highlighted"
        )),
        "Member's #[hide_field] should be inherited through Vec<Member>"
    );
}

// ──────────────────────────────────────────────
// 11. Decorators inherited through Option<Box<T>>
// ──────────────────────────────────────────────

#[derive(Serialize, SpytialDecorators)]
struct LinkedList {
    head: Option<Box<Node>>,
}

#[derive(Serialize, SpytialDecorators)]
#[attribute(field = "data")]
#[orientation(selector = "{x, y : Node | x->y in next}", directions = ["right"])]
struct Node {
    data: u32,
    next: Option<Box<Node>>,
}

#[test]
fn decorators_inherited_through_option_box() {
    let list_decs = LinkedList::decorators();

    assert!(
        list_decs.directives.iter().any(|d| matches!(
            d,
            Directive::Attribute(a) if a.attribute.field == "data"
        )),
        "Node's #[attribute] should be inherited through Option<Box<Node>>"
    );
    assert!(
        list_decs
            .constraints
            .iter()
            .any(|c| matches!(c, Constraint::Orientation(_))),
        "Node's #[orientation] should be inherited through Option<Box<Node>>"
    );
}

// ──────────────────────────────────────────────
// 12. Type without decorators → empty
// ──────────────────────────────────────────────

#[derive(Serialize, SpytialDecorators)]
struct Plain {
    x: u32,
}

#[test]
fn type_without_attributes_has_empty_decorators() {
    let decs = Plain::decorators();
    assert!(decs.constraints.is_empty());
    assert!(decs.directives.is_empty());
}

// ──────────────────────────────────────────────
// 13. Enum with derive has empty decorators
// ──────────────────────────────────────────────

#[derive(Serialize, SpytialDecorators)]
#[allow(dead_code)]
enum Direction {
    Up,
    Down,
}

#[test]
fn enum_derive_produces_empty_decorators() {
    let decs = Direction::decorators();
    assert!(decs.constraints.is_empty());
    assert!(decs.directives.is_empty());
}

// ──────────────────────────────────────────────
// 14. Multiple constraint types compose
// ──────────────────────────────────────────────

// `atom_color` is deprecated but kept here deliberately: this test counts how
// many constraints and directives a mixed set produces, and the legacy form is
// one of the shapes that has to keep landing in the right section.
#[derive(Serialize, SpytialDecorators)]
#[allow(deprecated)]
#[orientation(selector = "sel1", directions = ["left", "below"])]
#[align(selector = "sel2", direction = "horizontal")]
#[atom_color(selector = "sel3", value = "green")]
#[hide_atom(selector = "Foo")]
struct MultiAnnotated {
    x: u32,
}

#[test]
fn multiple_annotation_types_all_captured() {
    let decs = MultiAnnotated::decorators();

    assert_eq!(decs.constraints.len(), 3, "orientation + align + hide_atom");
    assert_eq!(decs.directives.len(), 1, "atom_color");

    assert!(decs
        .constraints
        .iter()
        .any(|c| matches!(c, Constraint::Orientation(_))));
    assert!(decs
        .constraints
        .iter()
        .any(|c| matches!(c, Constraint::Align(_))));
    assert!(decs
        .directives
        .iter()
        .any(|d| matches!(d, Directive::AtomStyle(_))));
    assert!(decs
        .constraints
        .iter()
        .any(|c| matches!(c, Constraint::HideAtom(_))));
}

// ──────────────────────────────────────────────
// 15. YAML round-trip preserves content
// ──────────────────────────────────────────────

#[test]
fn yaml_output_contains_all_decorator_fields() {
    let decs = MultiAnnotated::decorators();
    let yaml = to_yaml(&decs).unwrap();

    assert!(yaml.contains("sel1"), "orientation selector in yaml");
    assert!(yaml.contains("left"), "orientation direction in yaml");
    assert!(yaml.contains("below"), "orientation direction in yaml");
    assert!(yaml.contains("sel2"), "align selector in yaml");
    assert!(yaml.contains("horizontal"), "align direction in yaml");
    assert!(yaml.contains("sel3"), "atom_color selector in yaml");
    assert!(yaml.contains("green"), "atom_color value in yaml");
    assert!(yaml.contains("Foo"), "hide_atom selector in yaml");
}

// ──────────────────────────────────────────────
// 16. Deep nesting: A → B → C decorator chain
// ──────────────────────────────────────────────

#[derive(Serialize, SpytialDecorators)]
struct LevelA {
    b: LevelB,
}

#[derive(Serialize, SpytialDecorators)]
#[hide_field(field = "from_b")]
struct LevelB {
    c: LevelC,
}

#[derive(Serialize, SpytialDecorators)]
#[hide_field(field = "from_c")]
struct LevelC {
    val: u32,
}

#[test]
fn three_level_decorator_inheritance() {
    let a_decs = LevelA::decorators();

    let hidden: Vec<_> = a_decs
        .directives
        .iter()
        .filter_map(|d| match d {
            Directive::HideField(h) => Some(h.hide_field.field.as_str()),
            _ => None,
        })
        .collect();

    assert!(hidden.contains(&"from_b"), "B's directive should reach A");
    assert!(
        hidden.contains(&"from_c"),
        "C's directive should reach A through B"
    );
}

// ──────────────────────────────────────────────
// 17. Undecorated field types compile and are safe
//
// This is the key test for the DecoProbe mechanism:
// a struct with a field whose type does NOT derive
// SpytialDecorators should compile fine and just
// return empty decorators for that field type.
// ──────────────────────────────────────────────

/// A plain struct that does NOT derive SpytialDecorators.
/// Before the probe mechanism this would cause a compile error
/// if a decorated parent contained it.
#[derive(Serialize)]
struct Undecorated {
    val: u32,
}

#[derive(Serialize, SpytialDecorators)]
#[hide_field(field = "owner")]
struct ContainsUndecorated {
    data: Undecorated,
}

#[test]
fn undecorated_field_type_compiles_and_returns_only_own_decorators() {
    let decs = ContainsUndecorated::decorators();

    // ContainsUndecorated has its own directive, but Undecorated contributes nothing.
    assert_eq!(decs.directives.len(), 1);
    assert!(decs.directives.iter().any(|d| matches!(
        d,
        Directive::HideField(h) if h.hide_field.field == "owner"
    )));
    assert!(decs.constraints.is_empty());
}

#[derive(Serialize, SpytialDecorators)]
#[attribute(field = "name")]
struct MixedFields {
    name: String,
    plain: Undecorated,
    decorated: Child,
}

#[test]
fn mixed_decorated_and_undecorated_fields() {
    let decs = MixedFields::decorators();

    // MixedFields has its own #[attribute].
    // Undecorated contributes nothing (via probe fallback).
    // Child contributes its #[atom_color] and #[attribute].
    assert!(
        decs.directives.iter().any(|d| matches!(
            d,
            Directive::Attribute(a) if a.attribute.field == "name"
        )),
        "MixedFields' own attribute should be present"
    );
    assert!(
        decs.directives
            .iter()
            .any(|d| matches!(d, Directive::AtomStyle(_))),
        "Child's atom_color should be inherited"
    );
    assert!(
        decs.directives.iter().any(|d| matches!(
            d,
            Directive::Attribute(a) if a.attribute.field == "name"
        )),
        "Child's attribute should be inherited"
    );
}

// ──────────────────────────────────────────────
// 18. Data instance + decorators together
// ──────────────────────────────────────────────

#[test]
fn data_instance_and_decorators_agree_on_types() {
    let val = Parent {
        child: Child {
            name: "test".into(),
        },
    };
    let inst = export_json_instance(&val);
    let decs = Parent::decorators();

    // The data instance should contain atoms for both Parent and Child
    assert!(inst.atoms.iter().any(|a| a.r#type == "Parent"));
    assert!(inst.atoms.iter().any(|a| a.r#type == "Child"));

    // The decorator YAML should reference both types' selectors
    let yaml = to_yaml(&decs).unwrap();
    assert!(yaml.contains("Parent"), "decorator yaml references Parent");
    assert!(yaml.contains("Child"), "decorator yaml references Child");
}

// ──────────────────────────────────────────────
// 19. Same-named relations split by source type (issue #79, spytial-core 6.0)
// ──────────────────────────────────────────────
//
// A relation record is keyed by source type and name, and its id spells both:
// `Person.name`. spytial-core 6.0 merges records by id and never by name, and
// a selector on a name sees the union of every record carrying it, so two
// structs with a same-named field give two records with exact headers rather
// than one record widened to `atom`. (Before 6.0 the engine merged by name,
// which is why the header used to widen; tests/conformance.rs pins the union
// against the engine itself.)

#[derive(Serialize)]
struct Person {
    name: String,
}

#[derive(Serialize)]
struct Company {
    name: String,
}

#[derive(Serialize)]
struct BothNames {
    p: Person,
    c: Company,
}

#[derive(Serialize)]
struct BothNamesReversed {
    c: Company,
    p: Person,
}

#[test]
fn same_named_field_across_types_splits_into_typed_records() {
    let inst = export_json_instance(&BothNames {
        p: Person { name: "Ada".into() },
        c: Company {
            name: "Acme".into(),
        },
    });

    let person = relation_by_id(&inst, "Person.name");
    let company = relation_by_id(&inst, "Company.name");
    assert_eq!(relations_named(&inst, "name").len(), 2);

    // Both records carry the bare name a selector matches, and each keeps the
    // exact header of its own source instead of a shared one widened to atom.
    assert_eq!(person.name, "name");
    assert_eq!(company.name, "name");
    assert_eq!(person.types, vec!["Person", "atom"]);
    assert_eq!(company.types, vec!["Company", "atom"]);

    assert_eq!(person.tuples.len(), 1);
    assert_eq!(company.tuples.len(), 1);
    assert_eq!(person.tuples[0].atoms[0], atom_by_type(&inst, "Person").id);
    assert_eq!(
        company.tuples[0].atoms[0],
        atom_by_type(&inst, "Company").id
    );
    assert_eq!(person.tuples[0].types, vec!["Person", "atom"]);
    assert_eq!(company.tuples[0].types, vec!["Company", "atom"]);
}

#[test]
fn records_are_independent_of_serialization_order() {
    let forward = export_json_instance(&BothNames {
        p: Person { name: "Ada".into() },
        c: Company {
            name: "Acme".into(),
        },
    });
    let reversed = export_json_instance(&BothNamesReversed {
        c: Company {
            name: "Acme".into(),
        },
        p: Person { name: "Ada".into() },
    });

    // The roots are different types, so only the split `name` records are
    // comparable — and they must not depend on which source serialized first.
    let summary = |inst: &JsonDataInstance| {
        let mut v: Vec<(String, Vec<String>, usize)> = relations_named(inst, "name")
            .iter()
            .map(|r| (r.id.clone(), r.types.clone(), r.tuples.len()))
            .collect();
        v.sort();
        v
    };
    assert_eq!(summary(&forward), summary(&reversed));
    assert_eq!(summary(&forward).len(), 2);
}

#[test]
fn records_are_emitted_in_first_seen_order() {
    let inst = export_json_instance(&BothNames {
        p: Person { name: "Ada".into() },
        c: Company {
            name: "Acme".into(),
        },
    });
    let ids: Vec<&str> = inst.relations.iter().map(|r| r.id.as_str()).collect();
    // The root's `p` is serialized first, then Person's `name`, then `c`, then
    // Company's `name` — a relation appears where its source first serialized.
    assert_eq!(
        ids,
        vec!["Person.name", "BothNames.p", "Company.name", "BothNames.c"]
    );
}

#[test]
fn single_source_type_record_is_exact() {
    let inst = export_json_instance(&Flat {
        name: "solo".into(),
        age: 1,
    });

    let name = relation(&inst, "name");
    assert_eq!(name.id, "Flat.name");
    assert_eq!(name.types, vec!["Flat", "atom"]);
    assert_eq!(relation(&inst, "age").id, "Flat.age");
    assert_eq!(relation(&inst, "age").types, vec!["Flat", "atom"]);
}

// Every record, built-in or field, is keyed the same way: its id is its
// source type and its name, and every one of its tuples has that source type.

#[derive(Serialize)]
enum Shape {
    Unit,
    Newtype(u8),
    Tuple(u8, u8),
    Named { w: u8 },
}

#[derive(Serialize)]
struct Meters(f64);

#[derive(Serialize)]
struct Kitchen {
    xs: Vec<u8>,
    pair: (u8, bool),
    m: std::collections::BTreeMap<String, u8>,
    o: Option<Option<u8>>,
    d: Meters,
    shapes: Vec<Shape>,
}

#[test]
fn every_record_id_spells_its_source_type_and_name() {
    let inst = export_json_instance(&Kitchen {
        xs: vec![1, 2],
        pair: (3, true),
        m: [("k".to_string(), 4)].into_iter().collect(),
        // `Some(None)` is the one shape that keeps a `Some` wrapper atom.
        o: Some(None),
        d: Meters(1.5),
        shapes: vec![
            Shape::Unit,
            Shape::Newtype(5),
            Shape::Tuple(6, 7),
            Shape::Named { w: 8 },
        ],
    });

    let mut seen = std::collections::HashSet::new();
    for rel in &inst.relations {
        assert_eq!(
            rel.id,
            format!("{}.{}", rel.types[0], rel.name),
            "record {:?}: id must be its source type and name",
            rel.id
        );
        assert!(
            seen.insert(rel.id.as_str()),
            "duplicate record id {:?}",
            rel.id
        );
        for tuple in &rel.tuples {
            assert_eq!(
                tuple.types[0], rel.types[0],
                "record {:?}: every tuple shares the record's source type",
                rel.id
            );
            assert_eq!(atom_by_id(&inst, &tuple.atoms[0]).r#type, rel.types[0]);
        }
    }

    // Fields and built-ins alike, from each emitter.
    for id in [
        "Kitchen.xs",
        "sequence.idx",
        "tuple.idx",
        "map.map_entry",
        "Some.value",
        "newtype_struct.value",
        "Shape.variant_value",
        "Shape.idx",
        "Shape.w",
    ] {
        relation_by_id(&inst, id);
    }
    // `xs` and `shapes` are both sequences, so their positions share a record.
    assert_eq!(relation_by_id(&inst, "sequence.idx").tuples.len(), 2 + 4);
}

// A user field named like a built-in relation no longer shares a record with
// it: the sources differ, so the ids do, and neither header is widened or
// mixed-arity.

#[derive(Serialize)]
struct HasIdx {
    idx: u32,
}

#[derive(Serialize)]
struct FieldShadowsBuiltins {
    a: HasIdx,   // field record HasIdx.idx, binary
    b: Vec<u32>, // built-in record sequence.idx, ternary
    c: Inner,    // field record Inner.value
    d: Meters,   // built-in record newtype_struct.value
}

#[test]
fn field_named_like_a_builtin_gets_its_own_record() {
    let inst = export_json_instance(&FieldShadowsBuiltins {
        a: HasIdx { idx: 9 },
        b: vec![10, 11],
        c: Inner { value: 7 },
        d: Meters(1.5),
    });

    let field_idx = relation_by_id(&inst, "HasIdx.idx");
    let seq_idx = relation_by_id(&inst, "sequence.idx");
    assert_eq!(field_idx.types, vec!["HasIdx", "atom"]);
    assert_eq!(field_idx.tuples.len(), 1);
    assert_eq!(seq_idx.types, vec!["sequence", "u64", "atom"]);
    assert_eq!(seq_idx.tuples.len(), 2);
    assert_eq!(relations_named(&inst, "idx").len(), 2);

    assert_eq!(
        relation_by_id(&inst, "Inner.value").types,
        vec!["Inner", "atom"]
    );
    assert_eq!(
        relation_by_id(&inst, "newtype_struct.value").types,
        vec!["newtype_struct", "atom"]
    );
    assert_eq!(relations_named(&inst, "value").len(), 2);
}

// A `#[serde(rename)]` can put a dot in a type or field name. The id must
// still be unique to the (source type, name) pair, because spytial-core
// merges records by id: with a bare join, type `A.B` + field `c` and type
// `A` + field `B.c` would both be `A.B.c`, and the second's tuples would be
// filed under the first's name.

#[derive(Serialize)]
#[serde(rename = "A.B")]
struct DottedType {
    c: u8,
}

#[derive(Serialize)]
#[serde(rename = "A")]
struct DottedField {
    #[serde(rename = "B.c")]
    b_c: u8,
}

#[derive(Serialize)]
struct DotCollision {
    x: DottedType,
    y: DottedField,
}

#[test]
fn dots_in_serde_names_do_not_collide_record_ids() {
    let inst = export_json_instance(&DotCollision {
        x: DottedType { c: 1 },
        y: DottedField { b_c: 2 },
    });

    let c = relation_by_id(&inst, r"A\.B.c");
    let bc = relation_by_id(&inst, r"A.B\.c");
    assert_eq!(c.name, "c");
    assert_eq!(c.types, vec!["A.B", "atom"]);
    assert_eq!(c.tuples.len(), 1);
    assert_eq!(bc.name, "B.c");
    assert_eq!(bc.types, vec!["A", "atom"]);
    assert_eq!(bc.tuples.len(), 1);

    let ids: std::collections::HashSet<&str> =
        inst.relations.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids.len(), inst.relations.len(), "every record id is unique");
}

// The one mixed-arity record left. An enum's variants all have the enum as
// their source type, so a tuple variant's ternary `idx` and a struct variant's
// binary field named `idx` land in one record, `Payload.idx`. Its header joins
// the common prefix and keeps the longest arity, and every tuple stays exact.

#[derive(Serialize)]
enum Payload {
    Positional(u32, u32),
    Named { idx: u32 },
}

#[derive(Serialize)]
struct MixedArity {
    a: Payload,
    b: Payload,
}

#[test]
fn enum_variants_sharing_a_relation_name_keep_one_ragged_record() {
    let inst = export_json_instance(&MixedArity {
        a: Payload::Named { idx: 9 },
        b: Payload::Positional(10, 11),
    });

    assert_eq!(relations_named(&inst, "idx").len(), 1);
    let rel = relation_by_id(&inst, "Payload.idx");
    assert_eq!(rel.tuples.len(), 3);
    // Position 0 is the enum in every tuple; the rest disagree and widen.
    assert_eq!(rel.types, vec!["Payload", "atom", "atom"]);

    let field_tuples: Vec<_> = rel.tuples.iter().filter(|t| t.atoms.len() == 2).collect();
    let seq_tuples: Vec<_> = rel.tuples.iter().filter(|t| t.atoms.len() == 3).collect();
    assert_eq!(field_tuples.len(), 1);
    assert_eq!(seq_tuples.len(), 2);
    assert_eq!(field_tuples[0].types, vec!["Payload", "atom"]);
    for tuple in seq_tuples {
        assert_eq!(tuple.types, vec!["Payload", "u64", "atom"]);
    }
}

#[test]
fn ragged_record_orders_longest_tuples_first() {
    // The ordering was introduced for spytial-core 4.x, whose normalizer kept a
    // relation header only when its length equaled the *first* tuple's arity.
    // Since 5.2.1 a mixed-arity record's header is replaced with an empty one
    // whatever the order, so the engine no longer depends on this — but the
    // emitted datum is still a contract of its own: the header describes the
    // longest tuple, a longest tuple comes first, and neither depends on which
    // variant serialized first.
    let forward = export_json_instance(&MixedArity {
        a: Payload::Named { idx: 9 },
        b: Payload::Positional(10, 11),
    });
    let reversed = export_json_instance(&MixedArity {
        a: Payload::Positional(10, 11),
        b: Payload::Named { idx: 9 },
    });

    for inst in [&forward, &reversed] {
        for rel in &inst.relations {
            assert_eq!(
                rel.types.len(),
                rel.tuples[0].types.len(),
                "record {:?}: header arity must match the first tuple's",
                rel.id
            );
        }
    }
    assert_eq!(
        relation_by_id(&forward, "Payload.idx").types,
        relation_by_id(&reversed, "Payload.idx").types
    );
}
