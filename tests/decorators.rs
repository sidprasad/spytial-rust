use serde::Serialize;
use spytial::spytial_annotations::{
    to_yaml, Constraint, Directive, GroupParams, HasSpytialDecorators,
    SpytialDecorators as SpytialDecoratorsType, SpytialDecoratorsBuilder,
};
use spytial::SpytialDecorators;

#[derive(Serialize, SpytialDecorators)]
#[align(selector = "peer", direction = "horizontal")]
#[orientation(selector = "peer", directions = ["right"])]
#[flag(name = "important")]
struct DerivedNode {
    id: u32,
}

#[test]
fn derive_macro_emits_align_and_existing_decorators() {
    let decorators = DerivedNode::decorators();

    assert!(decorators.constraints.iter().any(|constraint| {
        matches!(constraint, Constraint::Align(align)
            if align.align.selector == "peer" && align.align.direction == "horizontal")
    }));
    assert!(decorators.constraints.iter().any(|constraint| {
        matches!(constraint, Constraint::Orientation(orientation)
            if orientation.orientation.selector == "peer"
            && orientation.orientation.directions == vec!["right".to_string()])
    }));
    assert!(decorators.directives.iter().any(|directive| {
        matches!(directive, Directive::Flag(flag) if flag.flag == "important")
    }));

    let yaml = to_yaml(&decorators).unwrap();
    assert!(yaml.contains("align:"));
    assert!(yaml.contains("direction: horizontal"));
    assert!(yaml.contains("orientation:"));
    assert!(yaml.contains("flag: important"));
}

#[derive(Serialize, SpytialDecorators)]
#[tag(to_tag = "Person", name = "status", value = "Person.status")]
struct TaggedPerson {
    name: String,
    status: String,
}

#[test]
fn tag_directive_single() {
    let decorators = TaggedPerson::decorators();

    let tag = decorators
        .directives
        .iter()
        .find_map(|d| match d {
            Directive::Tag(t) => Some(t),
            _ => None,
        })
        .expect("expected a Tag directive");

    assert_eq!(tag.tag.to_tag, "Person");
    assert_eq!(tag.tag.name, "status");
    assert_eq!(tag.tag.value, "Person.status");

    let yaml = to_yaml(&decorators).unwrap();
    assert!(yaml.contains("tag:"));
    assert!(yaml.contains("toTag: Person"));
    assert!(yaml.contains("name: status"));
    assert!(yaml.contains("value: Person.status"));
}

#[derive(Serialize, SpytialDecorators)]
#[tag(to_tag = "Person", name = "age", value = "Person.age")]
#[tag(to_tag = "Car", name = "owner", value = "Car.ownedBy")]
struct MultiTagged {
    id: u32,
}

#[derive(Serialize, SpytialDecorators)]
#[edge_style(field = "left", value = "#000000")]
struct EdgeStyledMinimal {
    id: u32,
}

#[test]
fn edge_style_directive_minimal() {
    let decorators = EdgeStyledMinimal::decorators();

    let edge = decorators
        .directives
        .iter()
        .find_map(|d| match d {
            Directive::EdgeStyle(e) => Some(&e.edge_style),
            _ => None,
        })
        .expect("expected an EdgeStyle directive");

    assert_eq!(edge.field, "left");
    assert_eq!(edge.value, "#000000");
    assert!(edge.style.is_none());
    assert!(edge.weight.is_none());
    assert!(edge.show_label.is_none());
    assert!(edge.hidden.is_none());
    assert!(edge.filter.is_none());
    assert!(edge.selector.is_none());

    let yaml = to_yaml(&decorators).unwrap();
    assert!(yaml.contains("edgeColor:"));
    assert!(yaml.contains("field: left"));
    // Optional fields are skipped when None.
    assert!(!yaml.contains("style:"));
    assert!(!yaml.contains("weight:"));
    assert!(!yaml.contains("showLabel:"));
    assert!(!yaml.contains("hidden:"));
}

#[derive(Serialize, SpytialDecorators)]
#[edge_style(
    field = "right",
    value = "blue",
    style = "dashed",
    weight = 2.5,
    show_label = false,
    hidden = true,
    filter = "Node3 -> Node1",
    selector = "Tree"
)]
struct EdgeStyledAllOptions {
    id: u32,
}

#[test]
fn edge_style_directive_all_options() {
    let decorators = EdgeStyledAllOptions::decorators();

    let edge = decorators
        .directives
        .iter()
        .find_map(|d| match d {
            Directive::EdgeStyle(e) => Some(&e.edge_style),
            _ => None,
        })
        .expect("expected an EdgeStyle directive");

    assert_eq!(edge.field, "right");
    assert_eq!(edge.value, "blue");
    assert_eq!(edge.style.as_deref(), Some("dashed"));
    assert_eq!(edge.weight, Some(2.5));
    assert_eq!(edge.show_label, Some(false));
    assert_eq!(edge.hidden, Some(true));
    assert_eq!(edge.filter.as_deref(), Some("Node3 -> Node1"));
    assert_eq!(edge.selector.as_deref(), Some("Tree"));

    let yaml = to_yaml(&decorators).unwrap();
    assert!(yaml.contains("edgeColor:"));
    assert!(yaml.contains("style: dashed"));
    assert!(yaml.contains("weight: 2.5"));
    assert!(yaml.contains("showLabel: false"));
    assert!(yaml.contains("hidden: true"));
    assert!(yaml.contains("filter: Node3"));
}

#[test]
fn tag_directive_multiple() {
    let decorators = MultiTagged::decorators();

    let tags: Vec<_> = decorators
        .directives
        .iter()
        .filter_map(|d| match d {
            Directive::Tag(t) => Some(&t.tag),
            _ => None,
        })
        .collect();

    assert_eq!(tags.len(), 2);
    assert!(tags.iter().any(|t| t.to_tag == "Person" && t.name == "age"));
    assert!(tags.iter().any(|t| t.to_tag == "Car" && t.name == "owner"));
}

#[derive(Serialize, SpytialDecorators)]
#[orientation(selector = "Person", directions = ["above"], negated = true)]
#[align(selector = "Person", direction = "horizontal", negated = true)]
#[cyclic(selector = "next", direction = "clockwise", negated = true)]
#[group(selector = "Foo", name = "fooGroup", negated = true)]
#[allow(clippy::duplicated_attributes)]
#[group(field = "rel", group_on = 0, add_to_group = 1, negated = true)]
struct AllNegated {
    id: u32,
}

#[test]
fn negated_constraints_emit_hold_never() {
    let decorators = AllNegated::decorators();

    let orientation = decorators
        .constraints
        .iter()
        .find_map(|c| match c {
            Constraint::Orientation(o) => Some(&o.orientation),
            _ => None,
        })
        .expect("orientation");
    assert!(orientation.negated);

    let align = decorators
        .constraints
        .iter()
        .find_map(|c| match c {
            Constraint::Align(a) => Some(&a.align),
            _ => None,
        })
        .expect("align");
    assert!(align.negated);

    let cyclic = decorators
        .constraints
        .iter()
        .find_map(|c| match c {
            Constraint::Cyclic(c) => Some(&c.cyclic),
            _ => None,
        })
        .expect("cyclic");
    assert!(cyclic.negated);

    let mut group_selector_negated = false;
    let mut group_field_negated = false;
    for c in &decorators.constraints {
        if let Constraint::Group(g) = c {
            match &g.group {
                GroupParams::SelectorBased { negated, .. } if *negated => {
                    group_selector_negated = true;
                }
                GroupParams::FieldBased { negated, .. } if *negated => {
                    group_field_negated = true;
                }
                _ => {}
            }
        }
    }
    assert!(
        group_selector_negated,
        "expected negated selector-based group"
    );
    assert!(group_field_negated, "expected negated field-based group");

    // Wire-format: negation surfaces as `hold: never` inside each inner
    // constraint object (matching spytial-core's parser).
    let yaml = to_yaml(&decorators).unwrap();
    let hold_never_count = yaml.matches("hold: never").count();
    assert_eq!(
        hold_never_count, 5,
        "expected 5 `hold: never` entries (one per negated constraint), got {hold_never_count}\n{yaml}"
    );
}

#[derive(Serialize, SpytialDecorators)]
#[orientation(selector = "Person", directions = ["above"])]
#[align(selector = "Person", direction = "horizontal")]
#[cyclic(selector = "next", direction = "clockwise")]
struct AllPositive {
    id: u32,
}

#[test]
fn positive_constraints_omit_hold_field() {
    let decorators = AllPositive::decorators();

    let yaml = to_yaml(&decorators).unwrap();
    assert!(
        !yaml.contains("hold:"),
        "positive constraints should not emit `hold` at all, got:\n{yaml}"
    );

    // Sanity-check: negated flags are all false.
    for c in &decorators.constraints {
        match c {
            Constraint::Orientation(o) => assert!(!o.orientation.negated),
            Constraint::Align(a) => assert!(!a.align.negated),
            Constraint::Cyclic(c) => assert!(!c.cyclic.negated),
            Constraint::Group(_) => {}
        }
    }
}

#[test]
fn negated_constraint_round_trips_through_yaml() {
    // Hand-build a single negated orientation, serialize, then deserialize
    // and verify negated survives. Matches spytial-core's `hold: never`
    // wire form.
    let original = SpytialDecoratorsBuilder::new()
        .orientation("r", vec!["above"], true)
        .build();

    let yaml = to_yaml(&original).unwrap();
    assert!(
        yaml.contains("hold: never"),
        "expected hold: never in:\n{yaml}"
    );

    let parsed: SpytialDecoratorsType = serde_yaml_ng::from_str(&yaml).unwrap();
    assert_eq!(parsed, original);

    // And a spytial-core-shaped YAML with the inner `hold: never` should
    // round-trip into a `negated == true` constraint.
    let core_yaml = r#"
constraints:
  - orientation:
      selector: r
      directions:
        - above
      hold: never
directives: []
"#;
    let from_core: SpytialDecoratorsType = serde_yaml_ng::from_str(core_yaml).unwrap();
    assert_eq!(from_core.constraints.len(), 1);
    if let Constraint::Orientation(o) = &from_core.constraints[0] {
        assert!(o.orientation.negated);
        assert_eq!(o.orientation.selector, "r");
        assert_eq!(o.orientation.directions, vec!["above".to_string()]);
    } else {
        panic!("expected orientation, got {:?}", from_core.constraints[0]);
    }
}
