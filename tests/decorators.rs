use serde::Serialize;
use spytial::spytial_annotations::{
    get_type_decorators, to_yaml, Constraint, Directive, DrawEnd, GroupParams,
    HasSpytialDecorators, IconPlacement, InferredEdgeDraw,
    SpytialDecorators as SpytialDecoratorsType, SpytialDecoratorsBuilder,
};
use spytial::SpytialDecorators;

#[derive(Serialize, SpytialDecorators)]
#[align(selector = "peer", direction = "horizontal")]
#[orientation(selector = "peer", directions = ["right"])]
#[flag(name = "hideDisconnected")]
struct DerivedNode {
    id: u32,
}

#[derive(Serialize, SpytialDecorators)]
#[hide_atom(selector = "LINK_TIME_ONLY_REGISTRY_PROBE")]
struct LinkTimeOnlyRegistryProbe;

#[test]
fn public_registry_reports_only_runtime_registrations() {
    assert!(get_type_decorators("LinkTimeOnlyRegistryProbe").is_none());

    let decorators = LinkTimeOnlyRegistryProbe::decorators();
    assert!(decorators
        .constraints
        .iter()
        .any(|constraint| matches!(constraint, Constraint::HideAtom(_))));
    assert!(get_type_decorators("LinkTimeOnlyRegistryProbe").is_some());
    assert!(get_type_decorators(std::any::type_name::<LinkTimeOnlyRegistryProbe>()).is_some());
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
        matches!(directive, Directive::Flag(flag) if flag.flag == "hideDisconnected")
    }));

    let yaml = to_yaml(&decorators).unwrap();
    assert!(yaml.contains("align:"));
    assert!(yaml.contains("direction: horizontal"));
    assert!(yaml.contains("orientation:"));
    assert!(yaml.contains("flag: hideDisconnected"));
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
#[edge_style(field = "left", line_style(color = "#000000"))]
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
    let line = edge
        .line_style
        .as_ref()
        .expect("expected a lineStyle block");
    assert_eq!(line.color.as_deref(), Some("#000000"));
    assert!(line.pattern.is_none());
    assert!(line.weight.is_none());
    assert!(edge.text_style.is_none());
    assert!(edge.show_label.is_none());
    assert!(edge.hidden.is_none());
    assert!(edge.filter.is_none());
    assert!(edge.selector.is_none());

    let yaml = to_yaml(&decorators).unwrap();
    assert!(yaml.contains("edgeStyle:"));
    assert!(yaml.contains("field: left"));
    assert!(yaml.contains("lineStyle:"));
    assert!(yaml.contains("color: '#000000'"));
    // Optional leaves are skipped when None.
    assert!(!yaml.contains("pattern:"));
    assert!(!yaml.contains("weight:"));
    assert!(!yaml.contains("showLabel:"));
    assert!(!yaml.contains("hidden:"));
    assert!(!yaml.contains("textStyle:"));
}

#[derive(Serialize, SpytialDecorators)]
#[edge_style(field = "left")]
struct EdgeStyledBareLegacy {
    id: u32,
}

#[derive(Serialize, SpytialDecorators)]
#[edge_style(field = "right", show_label = false)]
struct EdgeStyledFlagsOnlyLegacy {
    id: u32,
}

#[test]
fn bare_edge_style_keeps_legacy_blue_default() {
    // 0.1's flat parser defaulted `value` to "blue", so a styleless
    // #[edge_style] (no legacy keys, no blocks) must keep drawing a blue
    // line after the 3.x migration. Writing a block opts out of the default.
    for (decorators, field) in [
        (EdgeStyledBareLegacy::decorators(), "left"),
        (EdgeStyledFlagsOnlyLegacy::decorators(), "right"),
    ] {
        let edge = decorators
            .directives
            .iter()
            .find_map(|d| match d {
                Directive::EdgeStyle(e) => Some(&e.edge_style),
                _ => None,
            })
            .expect("expected an EdgeStyle directive");
        assert_eq!(edge.field, field);
        let line = edge
            .line_style
            .as_ref()
            .expect("styleless legacy form defaults a lineStyle block");
        assert_eq!(line.color.as_deref(), Some("blue"));
        assert!(line.pattern.is_none());
    }

    // The flags-only form still carries its flag alongside the default.
    let flags_only = EdgeStyledFlagsOnlyLegacy::decorators();
    let yaml = to_yaml(&flags_only).unwrap();
    assert!(yaml.contains("color: blue"));
    assert!(yaml.contains("showLabel: false"));
}

#[derive(Serialize, SpytialDecorators)]
#[edge_style(
    field = "right",
    line_style(color = "blue", pattern = "dashed", weight = 2.5),
    text_style(size = "small", color = "gray"),
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
    let line = edge
        .line_style
        .as_ref()
        .expect("expected a lineStyle block");
    assert_eq!(line.color.as_deref(), Some("blue"));
    assert_eq!(
        line.pattern,
        Some(spytial::spytial_annotations::LinePattern::Dashed)
    );
    assert_eq!(line.weight, Some(2.5));
    let text = edge
        .text_style
        .as_ref()
        .expect("expected a textStyle block");
    assert_eq!(
        text.size,
        Some(spytial::spytial_annotations::TextSize::Small)
    );
    assert_eq!(text.color.as_deref(), Some("gray"));
    assert_eq!(edge.show_label, Some(false));
    assert_eq!(edge.hidden, Some(true));
    assert_eq!(edge.filter.as_deref(), Some("Node3 -> Node1"));
    assert_eq!(edge.selector.as_deref(), Some("Tree"));

    let yaml = to_yaml(&decorators).unwrap();
    assert!(yaml.contains("edgeStyle:"));
    assert!(yaml.contains("lineStyle:"));
    assert!(yaml.contains("pattern: dashed"));
    assert!(yaml.contains("weight: 2.5"));
    assert!(yaml.contains("textStyle:"));
    assert!(yaml.contains("size: small"));
    assert!(yaml.contains("showLabel: false"));
    assert!(yaml.contains("hidden: true"));
    assert!(yaml.contains("filter: Node3"));
}

#[derive(Serialize, SpytialDecorators)]
#[allow(deprecated)]
#[edge_style(
    field = "legacy_edge",
    value = "seagreen",
    style = "Dotted",
    weight = 3.0
)]
struct EdgeStyledLegacyFlat {
    id: u32,
}

#[test]
fn edge_style_legacy_flat_keys_desugar_to_blocks() {
    // The 2.x flat keys keep working and rewrite onto the 3.x blocks:
    // value -> lineStyle.color, style -> lineStyle.pattern (normalized like
    // spytial-core: trimmed + lowercased), weight -> lineStyle.weight.
    let decorators = EdgeStyledLegacyFlat::decorators();
    let yaml = to_yaml(&decorators).unwrap();
    assert!(yaml.contains("edgeStyle:"));
    assert!(!yaml.contains("edgeColor:"));
    assert!(yaml.contains("lineStyle:"));
    assert!(yaml.contains("color: seagreen"));
    assert!(yaml.contains("pattern: dotted"));
    assert!(yaml.contains("weight: 3.0"));
    assert!(!yaml.contains("value:"));
}

#[derive(Serialize, SpytialDecorators)]
#[atom_style(
    selector = "Node",
    border_style(color = "steelblue", width = 2.0),
    fill_style(color = "#eef6ff"),
    text_style(size = "large")
)]
struct AtomStyled {
    id: u32,
}

#[test]
fn atom_style_directive_blocks() {
    let decorators = AtomStyled::decorators();
    let yaml = to_yaml(&decorators).unwrap();
    assert!(yaml.contains("atomStyle:"));
    assert!(yaml.contains("selector: Node"));
    assert!(yaml.contains("borderStyle:"));
    assert!(yaml.contains("color: steelblue"));
    assert!(yaml.contains("width: 2.0"));
    assert!(yaml.contains("fillStyle:"));
    assert!(yaml.contains("textStyle:"));
    assert!(yaml.contains("size: large"));
}

#[derive(Serialize, SpytialDecorators)]
#[atom_style(
    selector = "{x : Node | @:(x.color) = \"Red\"}",
    border_style(color = "red")
)]
#[orientation(selector = "{x, y : Node | @:(x.tag) = \"a, b\"}", directions = ["right"])]
struct QuotedSelector {
    id: u32,
}

#[test]
fn selector_keeps_escaped_string_literals() {
    // simple-graph-query 3.0 reads a bare name as the empty relation, so string
    // comparands are quoted — which puts escaped quotes inside the selector.
    // They must survive extraction intact, including a comma inside the quotes.
    let decorators = QuotedSelector::decorators();

    assert!(decorators.directives.iter().any(|directive| {
        matches!(directive, Directive::AtomStyle(style)
            if style.atom_style.selector.as_deref() == Some("{x : Node | @:(x.color) = \"Red\"}"))
    }));
    assert!(decorators.constraints.iter().any(|constraint| {
        matches!(constraint, Constraint::Orientation(orientation)
            if orientation.orientation.selector == "{x, y : Node | @:(x.tag) = \"a, b\"}")
    }));

    let yaml = to_yaml(&decorators).unwrap();
    let parsed: SpytialDecoratorsType = serde_yaml_ng::from_str(&yaml).unwrap();
    assert!(parsed.directives.iter().any(|directive| {
        matches!(directive, Directive::AtomStyle(style)
            if style.atom_style.selector.as_deref() == Some("{x : Node | @:(x.color) = \"Red\"}"))
    }));
}

#[derive(Serialize, SpytialDecorators)]
#[atom_style(
    selector = r#"{x : Node | @:(x.color) = "Red"}"#,
    border_style(color = "red")
)]
#[hide_atom(selector = r"Color + u32")]
#[align(
    // The body reads like a `direction = "..."` pair, but it is the selector's
    // content — the real direction is the one outside the literal.
    selector = r#"{x, y : Node | @:(x.note) = "direction = "}"#,
    direction = "vertical"
)]
struct RawSelector {
    id: u32,
}

#[test]
fn selector_accepts_raw_string_literals() {
    // A raw string is the natural way to write a selector once the query needs
    // quotes of its own, so it has to parse as readily as the escaped form.
    let decorators = RawSelector::decorators();

    let style = decorators
        .directives
        .iter()
        .find_map(|directive| match directive {
            Directive::AtomStyle(style) => Some(&style.atom_style),
            _ => None,
        })
        .expect("atom_style directive");
    assert_eq!(
        style.selector.as_deref(),
        Some(r#"{x : Node | @:(x.color) = "Red"}"#)
    );
    // The selector's own quotes and parens must not swallow the sibling block.
    assert_eq!(
        style.border_style.as_ref().and_then(|b| b.color.as_deref()),
        Some("red")
    );

    assert!(decorators.constraints.iter().any(|constraint| {
        matches!(constraint, Constraint::HideAtom(hide) if hide.hide_atom.selector == "Color + u32")
    }));
    assert!(decorators.constraints.iter().any(|constraint| {
        matches!(constraint, Constraint::Align(align)
            if align.align.selector == r#"{x, y : Node | @:(x.note) = "direction = "}"#
            && align.align.direction == "vertical")
    }));
}

#[derive(Serialize, SpytialDecorators)]
#[atom_style(
    // Lexical torture, not a real query: an unbalanced paren inside the literal
    // must not unbalance the scan that finds `border_style`'s group.
    selector = r#"{x : Node | @:(x.label) = "(("}"#,
    border_style(color = "blue")
)]
struct RawSelectorUnbalancedParen {
    id: u32,
}

#[test]
fn raw_selector_parens_do_not_unbalance_group_scan() {
    let decorators = RawSelectorUnbalancedParen::decorators();
    let style = decorators
        .directives
        .iter()
        .find_map(|directive| match directive {
            Directive::AtomStyle(style) => Some(&style.atom_style),
            _ => None,
        })
        .expect("atom_style directive");
    assert_eq!(
        style.selector.as_deref(),
        Some(r#"{x : Node | @:(x.label) = "(("}"#)
    );
    assert_eq!(
        style.border_style.as_ref().and_then(|b| b.color.as_deref()),
        Some("blue")
    );
}

// Each selector below contains text that reads like one of the attribute's own
// keys. A key is only a key outside the literal.
#[derive(Serialize, SpytialDecorators)]
#[size(
    selector = r#"{x : Node | @:(x.label) = "width = 3"}"#,
    height = 77,
    width = 88
)]
struct KeyTextInSelectorSize {
    id: u32,
}

#[derive(Serialize, SpytialDecorators)]
#[orientation(
    selector = r#"{x, y : Node | @:(x.note) = "negated = false"}"#,
    directions = ["right"],
    negated = true
)]
struct KeyTextInSelectorOrientation {
    id: u32,
}

#[derive(Serialize, SpytialDecorators)]
#[group(selector = r#"{x, y : Node | @:(x.note) = "field = id"}"#, name = "g")]
struct KeyTextInSelectorGroup {
    id: u32,
}

#[test]
fn key_text_inside_a_selector_is_not_a_key() {
    // A number: the flat scan matched `width = ` inside the literal, failed to
    // parse `3"}"#`, and silently fell back to the default of 30.
    let size = KeyTextInSelectorSize::decorators()
        .constraints
        .iter()
        .find_map(|constraint| match constraint {
            Constraint::Size(size) => Some(size.size.clone()),
            _ => None,
        })
        .expect("size constraint");
    assert_eq!(size.height, 77);
    assert_eq!(size.width, 88);

    // A bool: same shape, and it dropped a `negated = true` that was really set.
    assert!(KeyTextInSelectorOrientation::decorators()
        .constraints
        .iter()
        .any(|constraint| {
            matches!(constraint, Constraint::Orientation(orientation)
                if orientation.orientation.negated)
        }));

    // The choice between `group`'s two shapes: `field = id` inside the selector
    // used to route a selector-based group to the field-based branch.
    let group = KeyTextInSelectorGroup::decorators()
        .constraints
        .iter()
        .find_map(|constraint| match constraint {
            Constraint::Group(group) => Some(group.group.clone()),
            _ => None,
        })
        .expect("group constraint");
    assert!(
        matches!(group, GroupParams::SelectorBased { ref name, .. } if name == "g"),
        "expected a selector-based group, got {group:?}"
    );
}

#[derive(Serialize, SpytialDecorators)]
#[atom_style(
    selector = r#"{x : Node |
    @:(x.note) = "a
b"}"#,
    border_style(color = "red")
)]
struct MultiLineRawSelector {
    id: u32,
}

#[test]
fn raw_selector_keeps_its_own_whitespace() {
    // Token text used to be flattened before extraction, which rewrote the
    // newlines a raw string can legitimately carry — including inside a quoted
    // comparand, where `"a\nb"` silently became `"a b"`.
    let decorators = MultiLineRawSelector::decorators();
    let selector = decorators
        .directives
        .iter()
        .find_map(|directive| match directive {
            Directive::AtomStyle(style) => style.atom_style.selector.clone(),
            _ => None,
        })
        .expect("atom_style selector");
    assert_eq!(
        selector, "{x : Node |\n    @:(x.note) = \"a\nb\"}",
        "the selector should be exactly what was written"
    );
}

#[derive(Serialize, SpytialDecorators)]
#[allow(deprecated)]
#[atom_color(selector = "Legacy", value = "crimson")]
struct AtomColorLegacy {
    id: u32,
}

#[test]
fn atom_color_desugars_to_border_style() {
    // Legacy atomColor colored the atom's border; the rewrite must preserve
    // that look (border, NOT fill).
    let decorators = AtomColorLegacy::decorators();
    let yaml = to_yaml(&decorators).unwrap();
    assert!(yaml.contains("atomStyle:"));
    assert!(!yaml.contains("atomColor:"));
    assert!(yaml.contains("borderStyle:"));
    assert!(yaml.contains("color: crimson"));
    assert!(!yaml.contains("fillStyle:"));
}

#[derive(Serialize, SpytialDecorators)]
#[inferred_edge(
    name = "ancestor",
    selector = "^parent",
    line_style(color = "gray", pattern = "dotted")
)]
struct InferredEdgeStyled {
    id: u32,
}

#[test]
fn inferred_edge_line_style_block() {
    let decorators = InferredEdgeStyled::decorators();
    let yaml = to_yaml(&decorators).unwrap();
    assert!(yaml.contains("inferredEdge:"));
    assert!(yaml.contains("name: ancestor"));
    assert!(yaml.contains("lineStyle:"));
    assert!(yaml.contains("pattern: dotted"));
    // No `draw` key at all when the attribute doesn't ask for one — an
    // inferredEdge that omits it runs atom to atom.
    assert!(!yaml.contains("draw:"));
}

#[derive(Serialize, SpytialDecorators)]
#[group(selector = "region", name = "regions")]
#[inferred_edge(
    name = "connected",
    selector = "connected",
    draw = "regions -> regions"
)]
#[inferred_edge(name = "manages", selector = "manages", draw = "_ -> regions")]
struct InferredEdgeDrawn {
    id: u32,
}

#[test]
fn inferred_edge_draw_serializes_as_the_yaml_scalar() {
    // spytial-core 3.2: `draw` reinterprets each endpoint onto a group hull.
    // The wire form is the scalar string spytial-core's parser splits on "->",
    // not a nested source/target mapping.
    let decorators = InferredEdgeDrawn::decorators();
    let yaml = to_yaml(&decorators).unwrap();
    assert!(yaml.contains("draw: regions -> regions"));
    assert!(yaml.contains("draw: _ -> regions"));
    // `InferredEdgeDraw`'s own fields are `source`/`target`; neither may leak
    // as a key. (`source:` does appear in the document — every rule carries
    // its `source` block — so the check is on the draw's other field.)
    assert!(!yaml.contains("target:"), "{yaml}");

    let draws: Vec<_> = decorators
        .directives
        .iter()
        .filter_map(|directive| match directive {
            Directive::InferredEdge(edge) => Some(edge.inferred_edge.draw.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        draws,
        vec![
            Some(InferredEdgeDraw::new(
                DrawEnd::Group("regions".to_string()),
                DrawEnd::Group("regions".to_string()),
            )),
            Some(InferredEdgeDraw::new(
                DrawEnd::Atom,
                DrawEnd::Group("regions".to_string()),
            )),
        ]
    );
}

#[test]
fn inferred_edge_draw_round_trips_through_yaml() {
    let built = SpytialDecoratorsBuilder::new()
        .inferred_edge_drawn(
            "connected",
            "connected",
            Some(InferredEdgeDraw::new(
                DrawEnd::Group("regions".to_string()),
                DrawEnd::Atom,
            )),
            None,
            None,
        )
        .build();
    let yaml = to_yaml(&built).unwrap();
    assert!(yaml.contains("draw: regions -> _"));

    let parsed: SpytialDecoratorsType = serde_yaml_ng::from_str(&yaml).unwrap();
    assert_eq!(parsed.directives, built.directives);
}

#[test]
fn inferred_edge_draw_rejects_malformed_scalars() {
    // Deserialization is the only path a `draw` string reaches at runtime —
    // the attribute form is gated at compile time by the macro instead.
    for raw in [
        "directives:\n- inferredEdge:\n    name: e\n    selector: s\n    draw: regions\n",
        "directives:\n- inferredEdge:\n    name: e\n    selector: s\n    draw: a -> b -> c\n",
        "directives:\n- inferredEdge:\n    name: e\n    selector: s\n    draw: ' -> regions'\n",
    ] {
        assert!(
            serde_yaml_ng::from_str::<SpytialDecoratorsType>(raw).is_err(),
            "expected {raw:?} to be rejected"
        );
    }
}

#[derive(Serialize, SpytialDecorators)]
#[attribute(field = "weight", text_style(size = "small"))]
#[tag(
    to_tag = "Person",
    name = "age",
    value = "Person.age",
    text_style(color = "gray")
)]
struct TextStyledLines {
    weight: u32,
}

#[test]
fn attribute_and_tag_text_style() {
    // spytial-core 3.1: attribute/tag lines take the shared textStyle block.
    let decorators = TextStyledLines::decorators();
    let yaml = to_yaml(&decorators).unwrap();
    assert!(yaml.contains("attribute:"));
    assert!(yaml.contains("size: small"));
    assert!(yaml.contains("tag:"));
    assert!(yaml.contains("color: gray"));
    assert!(!yaml.contains("textSize:"));
}

#[derive(Serialize, SpytialDecorators)]
#[group(
    selector = "Team.members",
    name = "Team",
    add_edge(
        points = "togroup",
        line_style(pattern = "dashed"),
        text_style(size = "small")
    ),
    text_style(color = "navy")
)]
struct GroupStyled {
    id: u32,
}

#[test]
fn group_add_edge_block_and_label_style() {
    let decorators = GroupStyled::decorators();
    let yaml = to_yaml(&decorators).unwrap();
    assert!(yaml.contains("group:"));
    assert!(yaml.contains("addEdge:"));
    assert!(yaml.contains("points: togroup"));
    assert!(yaml.contains("pattern: dashed"));
    // The group's own label styling is a sibling of addEdge.
    assert!(yaml.contains("color: navy"));
}

#[derive(Serialize, SpytialDecorators)]
#[group(
    selector = "Herd.animals",
    name = "Herd",
    add_edge = "fromgroup",
    show_label = false
)]
struct GroupBareAddEdge {
    id: u32,
}

#[test]
fn group_add_edge_bare_direction() {
    let decorators = GroupBareAddEdge::decorators();
    let yaml = to_yaml(&decorators).unwrap();
    assert!(yaml.contains("addEdge: fromgroup"));
    assert!(yaml.contains("showLabel: false"));
    assert!(!yaml.contains("points:"));
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

    let group_negated = decorators.constraints.iter().any(|c| match c {
        Constraint::Group(g) => {
            matches!(&g.group, GroupParams::SelectorBased { negated, .. } if *negated)
        }
        _ => false,
    });
    assert!(group_negated, "expected negated selector-based group");

    // Wire-format: negation surfaces as `hold: never` inside each inner
    // constraint object (matching spytial-core's parser).
    let yaml = to_yaml(&decorators).unwrap();
    let hold_never_count = yaml.matches("hold: never").count();
    assert_eq!(
        hold_never_count, 4,
        "expected 4 `hold: never` entries (one per negated constraint), got {hold_never_count}\n{yaml}"
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
            // `size` and `hideAtom` accept `hold` syntactically but ignore it,
            // so they carry no negation to check.
            Constraint::Size(_) | Constraint::HideAtom(_) => {}
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

// ──────────────────────────────────────────────
// spytial-core 4.2/4.3 surface: the iconStyle block and atomStyle's
// independent showLabel, which together replace the old `icon` directive.
// ──────────────────────────────────────────────

#[derive(Serialize, SpytialDecorators)]
#[atom_style(
    selector = "Person",
    icon_style(path = "bi:person-fill", placement = "badge", opacity = 0.4),
    show_label = false
)]
struct Badged {
    name: String,
}

#[test]
fn atom_style_carries_icon_style_and_show_label() {
    let decorators = Badged::decorators();
    let style = decorators
        .directives
        .iter()
        .find_map(|d| match d {
            Directive::AtomStyle(s) => Some(&s.atom_style),
            _ => None,
        })
        .expect("atom_style directive");

    let icon = style.icon_style.as_ref().expect("icon_style block");
    assert_eq!(icon.path.as_deref(), Some("bi:person-fill"));
    assert_eq!(icon.placement, Some(IconPlacement::Badge));
    assert_eq!(icon.opacity, Some(0.4));
    assert_eq!(style.show_label, Some(false));

    let yaml = to_yaml(&decorators).unwrap();
    assert!(
        yaml.contains("iconStyle:"),
        "expected iconStyle in:\n{yaml}"
    );
    assert!(yaml.contains("placement: badge"), "in:\n{yaml}");
    assert!(yaml.contains("showLabel: false"), "in:\n{yaml}");
}

#[derive(Serialize, SpytialDecorators)]
#[allow(deprecated)]
#[icon(selector = "Person", path = "person.png", show_labels = true)]
struct LegacyIcon {
    name: String,
}

#[test]
fn legacy_icon_rewrites_onto_atom_style() {
    let decorators = LegacyIcon::decorators();
    let style = decorators
        .directives
        .iter()
        .find_map(|d| match d {
            Directive::AtomStyle(s) => Some(&s.atom_style),
            _ => None,
        })
        .expect("icon should desugar to an atom_style directive");

    // `showLabels: true` splits into a badge beside a visible label.
    let icon = style.icon_style.as_ref().expect("icon_style block");
    assert_eq!(icon.path.as_deref(), Some("person.png"));
    assert_eq!(icon.placement, Some(IconPlacement::Badge));
    assert_eq!(style.show_label, Some(true));

    let yaml = to_yaml(&decorators).unwrap();
    assert!(
        !yaml.contains("icon:"),
        "the deprecated icon directive should not reach the wire:\n{yaml}"
    );
}

// ──────────────────────────────────────────────
// Keys the runtime always supported but the macro did not expose.
// ──────────────────────────────────────────────

#[derive(Serialize, SpytialDecorators)]
#[attribute(field = "role", selector = "Person", filter = "Manager")]
#[hide_field(field = "secret", selector = "Person", filter = "internal")]
struct Scoped {
    role: String,
    secret: String,
}

#[test]
fn attribute_and_hide_field_carry_selector_and_filter() {
    let decorators = Scoped::decorators();

    let attribute = decorators
        .directives
        .iter()
        .find_map(|d| match d {
            Directive::Attribute(a) => Some(&a.attribute),
            _ => None,
        })
        .expect("attribute directive");
    assert_eq!(attribute.field, "role");
    assert_eq!(attribute.selector.as_deref(), Some("Person"));
    assert_eq!(attribute.filter.as_deref(), Some("Manager"));

    let hidden = decorators
        .directives
        .iter()
        .find_map(|d| match d {
            Directive::HideField(h) => Some(&h.hide_field),
            _ => None,
        })
        .expect("hide_field directive");
    assert_eq!(hidden.field, "secret");
    assert_eq!(hidden.selector.as_deref(), Some("Person"));
    assert_eq!(hidden.filter.as_deref(), Some("internal"));
}

// ──────────────────────────────────────────────
// Defaults now come from spytial-core's manifest rather than from values
// that were never in the vocabulary.
// ──────────────────────────────────────────────

#[derive(Serialize, SpytialDecorators)]
#[cyclic(selector = "next")]
struct DefaultedCycle {
    id: u32,
}

#[test]
fn cyclic_defaults_to_the_manifest_direction() {
    let decorators = DefaultedCycle::decorators();
    let cyclic = decorators
        .constraints
        .iter()
        .find_map(|c| match c {
            Constraint::Cyclic(c) => Some(&c.cyclic),
            _ => None,
        })
        .expect("cyclic constraint");

    // Previously "up", which is not a cycle direction at all.
    assert_eq!(cyclic.direction, "clockwise");
}

#[derive(Serialize, SpytialDecorators)]
#[allow(deprecated)]
#[icon(selector = "Person", path = "person.png")]
struct BareLegacyIcon {
    name: String,
}

#[test]
fn bare_legacy_icon_uses_the_manifest_default() {
    // `showLabels` defaults to false upstream, so the rewrite is a full-box
    // icon with the label off. Defaulting it to true inverted both halves:
    // a corner badge with the label on.
    let decorators = BareLegacyIcon::decorators();
    let style = decorators
        .directives
        .iter()
        .find_map(|d| match d {
            Directive::AtomStyle(s) => Some(&s.atom_style),
            _ => None,
        })
        .expect("icon should desugar to an atom_style directive");

    let icon = style.icon_style.as_ref().expect("icon_style block");
    assert_eq!(icon.placement, Some(IconPlacement::Full));
    assert_eq!(style.show_label, Some(false));
}

#[derive(Serialize, SpytialDecorators)]
#[group(
    selector = "Region",
    name = "regions",
    add_edge(points = "togroup", line_style(color = "gray"))
)]
struct GroupedWithConnector {
    id: u32,
}

#[test]
fn add_edge_block_round_trips() {
    // The block form is validated like any other block now, so this also
    // guards that valid leaves are not rejected by that check.
    let decorators = GroupedWithConnector::decorators();
    let yaml = to_yaml(&decorators).unwrap();
    assert!(yaml.contains("addEdge:"), "in:\n{yaml}");
    assert!(yaml.contains("points: togroup"), "in:\n{yaml}");
    assert!(yaml.contains("color: gray"), "in:\n{yaml}");
}

// ──────────────────────────────────────────────
// source blocks (spytial-core 5.4)
// ──────────────────────────────────────────────

// `line!()` here is two lines above the first attribute on `Sourced`; the
// location assertion below depends on that distance, so keep them adjacent.
const SOURCED_ORIENTATION_LINE: u32 = line!() + 2;
#[derive(Serialize, SpytialDecorators)]
#[orientation(selector = "{x, y : Sourced | x->y in next}", directions = ["right"])]
#[hide_atom(selector = "None")]
#[flag(name = "hideDisconnected")]
struct Sourced {
    next: Option<Box<Sourced>>,
}

#[test]
fn derived_rules_carry_the_attribute_as_written_and_its_location() {
    let decorators = Sourced::decorators();

    let orientation = decorators
        .constraints
        .iter()
        .find_map(|c| match c {
            Constraint::Orientation(o) => Some(&o.orientation),
            _ => None,
        })
        .expect("orientation");
    let source = orientation
        .source
        .as_ref()
        .expect("the derive stamps a source");
    assert_eq!(
        source.text,
        r#"#[orientation(selector = "{x, y : Sourced | x->y in next}", directions = ["right"])]"#
    );
    assert_eq!(
        source.location.as_deref(),
        Some(format!("{}:{SOURCED_ORIENTATION_LINE}", file!()).as_str()),
        "location must be the attribute's own file:line, not the derive's",
    );

    let hide = decorators
        .constraints
        .iter()
        .find(|c| matches!(c, Constraint::HideAtom(_)))
        .expect("hide_atom");
    let source = hide.source().expect("stamped");
    assert_eq!(source.text, r#"#[hide_atom(selector = "None")]"#);
    assert_eq!(
        source.location.as_deref(),
        Some(format!("{}:{}", file!(), SOURCED_ORIENTATION_LINE + 1).as_str()),
    );

    // `flag` is a bare scalar in the wire format and has nowhere to carry one.
    let flag = decorators
        .directives
        .iter()
        .find(|d| matches!(d, Directive::Flag(_)))
        .expect("flag");
    assert!(flag.source().is_none());
}

#[test]
fn source_blocks_survive_the_yaml_round_trip() {
    let decorators = Sourced::decorators();
    let yaml = to_yaml(&decorators).unwrap();
    assert!(yaml.contains("source:"), "{yaml}");
    assert!(yaml.contains("text:"), "{yaml}");
    assert!(yaml.contains("location:"), "{yaml}");

    let parsed: SpytialDecoratorsType = serde_yaml_ng::from_str(&yaml).unwrap();
    assert_eq!(parsed, decorators);
}

#[test]
fn hand_built_rules_carry_no_source_unless_stamped() {
    let built = SpytialDecoratorsBuilder::new()
        .orientation("r", vec!["above"], false)
        .hide_atom("None")
        .source("#[hide_atom(selector = \"None\")]", Some("hand.rs:1"))
        .flag("hideDisconnected")
        .source("ignored: a flag has no block", None)
        .build();

    assert!(built.constraints[0].source().is_none());
    let stamped = built.constraints[1]
        .source()
        .expect("source() stamps the last rule");
    assert_eq!(stamped.text, "#[hide_atom(selector = \"None\")]");
    assert_eq!(stamped.location.as_deref(), Some("hand.rs:1"));
    assert!(built.directives[0].source().is_none());

    let yaml = to_yaml(&built).unwrap();
    assert_eq!(yaml.matches("source:").count(), 1, "{yaml}");
}

#[derive(Serialize, SpytialDecorators)]
#[hide_atom(selector = "Shared")]
struct SharedRuleOuter {
    inner: SharedRuleInner,
}

#[derive(Serialize, SpytialDecorators)]
#[hide_atom(selector = "Shared")]
struct SharedRuleInner {
    n: u32,
}

/// The same rule written on two types is one rule. spytial-core de-duplicates
/// identical rules keeping the first source; comparing sources here would emit
/// it twice and leave the engine to drop one.
#[test]
fn identical_rules_from_two_types_dedupe_ignoring_their_source() {
    let value = SharedRuleOuter {
        inner: SharedRuleInner { n: 1 },
    };
    let (_, decorators) = spytial::export::export_json_instance_with_decorators(&value, "");

    let hides: Vec<&Constraint> = decorators
        .constraints
        .iter()
        .filter(|c| matches!(c, Constraint::HideAtom(_)))
        .collect();
    assert_eq!(
        hides.len(),
        1,
        "one rule, not one per type:\n{decorators:#?}"
    );
    assert!(
        hides[0].source().is_some(),
        "the kept copy keeps its source, so the report can still cite a site"
    );

    let a = SharedRuleOuter::decorators().constraints[0].clone();
    let b = SharedRuleInner::decorators().constraints[0].clone();
    assert_ne!(a, b, "the two sites differ only in source");
    assert_eq!(a.without_source(), b.without_source());
}
