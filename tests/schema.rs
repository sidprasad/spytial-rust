//! Every spec the derive can emit has to validate against spytial-core's own
//! JSON Schema for a layout spec.
//!
//! This is the check the engine cannot do itself. spytial-core's parser
//! ignores unknown keys, unknown list items, and unknown fields inside a known
//! item — all silently — so a rule this crate emits under a misspelt key, in
//! the wrong section, or with a value outside the vocabulary renders a diagram
//! quietly missing it. The schema is strict where the parser is lenient, and
//! it ships beside the manifest the derive's tables are generated from, so the
//! two describe the same language at the same version.
//!
//! `tests/conformance.rs` asks what a spec *entails*; this file only asks
//! whether the spec is well-formed. Both are needed: a well-formed spec can
//! still say the wrong thing, and a spec that entails the right facts could
//! still carry a key the engine will one day reject.

use serde::Serialize;
use spytial::spytial_annotations::{to_yaml, HasSpytialDecorators};
use spytial::SpytialDecorators;

fn schema() -> serde_json::Value {
    serde_json::from_str(include_str!("../templates/vendor/spytial-spec.schema.json"))
        .expect("vendored schema is valid JSON")
}

fn validator() -> jsonschema::Validator {
    jsonschema::validator_for(&schema()).expect("vendored schema compiles")
}

/// The YAML `T` emits, as the JSON document the schema is written against.
fn emitted_document<T: HasSpytialDecorators>() -> (String, serde_json::Value) {
    let yaml = to_yaml(&T::decorators()).expect("decorators serialize");
    let doc: serde_json::Value = serde_yaml_ng::from_str(&yaml).expect("emitted YAML parses");
    (yaml, doc)
}

fn assert_valid<T: HasSpytialDecorators>(what: &str) {
    let (yaml, doc) = emitted_document::<T>();
    let errors: Vec<String> = validator()
        .iter_errors(&doc)
        .map(|e| format!("{} (at {})", e, e.instance_path()))
        .collect();
    assert!(
        errors.is_empty(),
        "{what}: the emitted spec does not validate against spytial-core's schema:\n  {}\n\nspec:\n{yaml}",
        errors.join("\n  "),
    );
}

// ── one decorated type per family, covering every attribute the derive accepts ──

#[derive(Serialize, SpytialDecorators)]
#[allow(clippy::duplicated_attributes)]
#[orientation(selector = "{x, y : Node | x->y in left}", directions = ["left", "below"])]
#[orientation(selector = "{x, y : Node | x->y in right}", directions = ["directlyRight", "right"], negated = true)]
#[align(selector = "peer", direction = "horizontal")]
#[cyclic(selector = "next", direction = "counterclockwise", negated = true)]
#[size(selector = "Node", height = 40, width = 60)]
#[hide_atom(selector = "Color + u32 + None")]
struct Constraints {
    left: Option<Box<Constraints>>,
}

#[derive(Serialize, SpytialDecorators)]
#[allow(clippy::duplicated_attributes)]
#[group(selector = "region", name = "regions")]
#[group(
    selector = "~works_in",
    name = "teams",
    add_edge = "togroup",
    show_label = false,
    negated = true
)]
#[group(
    selector = "Team.members",
    name = "styled",
    add_edge(
        points = "fromgroup",
        line_style(pattern = "dashed", weight = 1.5),
        text_style(size = "small")
    ),
    text_style(color = "navy")
)]
struct Groups {
    region: u32,
}

#[derive(Serialize, SpytialDecorators)]
#[allow(clippy::duplicated_attributes)]
#[attribute(field = "name")]
#[attribute(
    field = "age",
    selector = "Person",
    filter = "{p : Person, a : univ | p.age = a}",
    text_style(size = "small", color = "gray")
)]
#[flag(name = "hideDisconnected")]
#[flag(name = "hideDisconnectedBuiltIns")]
#[hide_field(field = "secret")]
#[hide_field(field = "extra", selector = "Person", filter = "extra")]
#[tag(to_tag = "Person", name = "age", value = "age")]
#[tag(
    to_tag = "Person",
    name = "member",
    value = "Person",
    text_style(size = "large")
)]
struct Labels {
    name: String,
    age: u32,
    secret: u32,
    extra: u32,
}

#[derive(Serialize, SpytialDecorators)]
#[allow(clippy::duplicated_attributes)]
#[atom_style(
    selector = "{x : Node | @:(x.color) = \"Red\"}",
    border_style(color = "red", width = 2.0)
)]
#[atom_style(
    fill_style(color = "#eef6ff"),
    icon_style(path = "person", placement = "badge", opacity = 0.4),
    text_style(size = "large"),
    show_label = false
)]
#[edge_style(
    field = "left",
    line_style(color = "blue", pattern = "dotted", weight = 2.0, highlight = "yellow"),
    text_style(size = "small", color = "black"),
    show_label = true,
    hidden = false
)]
#[edge_style(field = "right", selector = "Node", filter = "right")]
#[inferred_edge(name = "reachable", selector = "^left")]
#[inferred_edge(
    name = "manages",
    selector = "manages",
    draw = "_ -> regions",
    line_style(color = "gray", pattern = "dashed"),
    text_style(size = "small")
)]
#[group(selector = "region", name = "regions")]
struct Styles {
    left: u32,
    right: u32,
    manages: u32,
    region: u32,
}

/// The deprecated forms rewrite onto current ones; what they emit has to be
/// well-formed too, since the rewrite is this crate's, not the engine's.
#[derive(Serialize, SpytialDecorators)]
#[allow(deprecated)]
#[icon(selector = "Person", path = "person", show_labels = true)]
#[atom_color(selector = "Node", value = "red")]
#[edge_style(field = "left", value = "blue", style = "dashed", weight = 2.0)]
struct Legacy {
    left: u32,
}

#[derive(Serialize, SpytialDecorators)]
struct Undecorated {
    n: u32,
}

#[test]
fn constraints_validate() {
    assert_valid::<Constraints>("orientation/align/cyclic/size/hide_atom");
}

#[test]
fn groups_validate() {
    assert_valid::<Groups>("group, bare and styled add_edge");
}

#[test]
fn label_directives_validate() {
    assert_valid::<Labels>("attribute/flag/hide_field/tag");
}

#[test]
fn style_directives_validate() {
    assert_valid::<Styles>("atom_style/edge_style/inferred_edge");
}

#[test]
fn legacy_rewrites_validate() {
    assert_valid::<Legacy>("icon/atom_color/legacy edge_style");
}

#[test]
fn an_empty_spec_validates() {
    assert_valid::<Undecorated>("no decorators");
}

/// The `source` block the derive stamps is the one part of the emitted spec
/// that is not user-authored, so it is the part most worth checking against
/// the schema's shape for it.
#[test]
fn source_blocks_are_emitted_where_the_schema_allows_them() {
    let (yaml, doc) = emitted_document::<Constraints>();
    assert!(
        yaml.contains("source:"),
        "derive did not stamp a source:\n{yaml}"
    );
    let first = &doc["constraints"][0]["orientation"]["source"];
    assert!(
        first["text"]
            .as_str()
            .is_some_and(|t| t.starts_with("#[orientation(")),
        "source.text should be the attribute as written, got {first}"
    );
    assert!(
        first["location"]
            .as_str()
            .is_some_and(|l| l.contains("schema.rs:")),
        "source.location should be file:line, got {first}"
    );
}

/// Proof the validator is doing something: a spec with a key the language
/// does not have must be rejected. If the vendored schema ever stops
/// forbidding unknown keys, every test above passes vacuously.
#[test]
fn the_schema_rejects_an_unknown_key() {
    let doc = serde_json::json!({
        "constraints": [
            { "orientation": { "selector": "left", "directions": ["above"], "bogus": 1 } }
        ]
    });
    assert!(
        !validator().is_valid(&doc),
        "the schema accepted an unknown key; the tests in this file no longer prove anything"
    );
}

/// The schema and the manifest have to come from one spytial-core release —
/// they are vendored together, and the derive's tables come from the manifest.
#[test]
fn schema_and_manifest_are_from_the_vendored_release() {
    let schema = schema();
    let version_txt = include_str!("../templates/vendor/VERSION.txt");
    let core = schema["x-spytial-core-version"]
        .as_str()
        .expect("schema stamps its spytial-core version");
    assert!(
        version_txt.contains(core),
        "templates/vendor/VERSION.txt says {:?} but the schema is from spytial-core {core}; \
         run scripts/update-spytial-core.sh rather than copying files by hand",
        version_txt.trim(),
    );
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("../templates/vendor/spytial-language.json"))
            .expect("manifest is valid JSON");
    assert_eq!(
        schema["x-spytial-language-version"], manifest["languageVersion"],
        "schema and manifest describe different language versions"
    );
}
