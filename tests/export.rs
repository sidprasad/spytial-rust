//! Integration tests for the JSON data instance export.
//!
//! These tests verify the relational output format sent to spytial-core:
//! atoms, relations, type information, and how nested / annotated structs
//! compose.

use spytial::export::export_json_instance;
use spytial::jsondata::{IAtom, IRelation, JsonDataInstance};
use spytial::spytial_annotations::{to_yaml, Constraint, Directive, HasSpytialDecorators};
use spytial::SpytialDecorators;
use serde::Serialize;

// ──────────────────────────────────────────────
// Helpers
// ──────────────────────────────────────────────

/// Find a relation by name, panicking if absent.
fn relation<'a>(instance: &'a JsonDataInstance, name: &str) -> &'a IRelation {
    instance
        .relations
        .iter()
        .find(|r| r.name == name)
        .unwrap_or_else(|| {
            let names: Vec<&str> = instance.relations.iter().map(|r| r.name.as_str()).collect();
            panic!("no relation named {:?}; available: {:?}", name, names)
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
        name: "Alice".into(),
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
    assert_eq!(name_atom.label, "Alice");

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

    // Verify positional indices "0", "1", "2" appear in the tuples
    let indices: Vec<&str> = idx_rel.tuples.iter().map(|t| t.atoms[1].as_str()).collect();
    assert!(indices.contains(&"0"));
    assert!(indices.contains(&"1"));
    assert!(indices.contains(&"2"));
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
// 6. None singletons are deduplicated
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
// 7. Boolean singletons are deduplicated
// ──────────────────────────────────────────────

#[derive(Serialize)]
struct Flags {
    a: bool,
    b: bool,
}

#[test]
fn boolean_atoms_are_singletons() {
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

#[derive(Serialize, SpytialDecorators)]
#[atom_color(selector = "{x : Parent | true}", value = "blue")]
struct Parent {
    child: Child,
}

#[derive(Serialize, SpytialDecorators)]
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
        .filter(|d| matches!(d, Directive::AtomColor(_)))
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
        .filter(|d| matches!(d, Directive::AtomColor(_)))
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
#[flag(name = "highlighted")]
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
            Directive::Flag(f) if f.flag == "highlighted"
        )),
        "Member's #[flag] should be inherited through Vec<Member>"
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

#[derive(Serialize, SpytialDecorators)]
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

    assert_eq!(decs.constraints.len(), 2, "orientation + align");
    assert_eq!(decs.directives.len(), 2, "atom_color + hide_atom");

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
        .any(|d| matches!(d, Directive::AtomColor(_))));
    assert!(decs
        .directives
        .iter()
        .any(|d| matches!(d, Directive::HideAtom(_))));
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
#[flag(name = "from_b")]
struct LevelB {
    c: LevelC,
}

#[derive(Serialize, SpytialDecorators)]
#[flag(name = "from_c")]
struct LevelC {
    val: u32,
}

#[test]
fn three_level_decorator_inheritance() {
    let a_decs = LevelA::decorators();

    let flags: Vec<_> = a_decs
        .directives
        .iter()
        .filter_map(|d| match d {
            Directive::Flag(f) => Some(f.flag.as_str()),
            _ => None,
        })
        .collect();

    assert!(flags.contains(&"from_b"), "B's flag should reach A");
    assert!(
        flags.contains(&"from_c"),
        "C's flag should reach A through B"
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
#[flag(name = "owner")]
struct ContainsUndecorated {
    data: Undecorated,
}

#[test]
fn undecorated_field_type_compiles_and_returns_only_own_decorators() {
    let decs = ContainsUndecorated::decorators();

    // ContainsUndecorated has its own #[flag], but Undecorated contributes nothing.
    assert_eq!(decs.directives.len(), 1);
    assert!(decs.directives.iter().any(|d| matches!(
        d,
        Directive::Flag(f) if f.flag == "owner"
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
            .any(|d| matches!(d, Directive::AtomColor(_))),
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
