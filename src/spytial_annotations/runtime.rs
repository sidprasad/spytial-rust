use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::HashMap;
use std::sync::LazyLock;
use std::sync::Mutex;

/// Wire-format helpers for the `negated` flag on constraints.
///
/// `spytial-core` represents constraint negation as `hold: never` inside the
/// inner constraint object (a positive constraint omits the `hold` key
/// entirely). On the Rust side we keep an ergonomic `negated: bool`, and
/// these helpers translate to/from the `hold` string at serialization
/// boundaries.
mod hold_serde {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(negated: &bool, ser: S) -> Result<S::Ok, S::Error> {
        // Caller is `skip_serializing_if = "is_not_negated"` — this only runs
        // when `*negated == true`, so always emit "never".
        debug_assert!(*negated);
        ser.serialize_str("never")
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
        let s: Option<String> = Option::deserialize(d)?;
        Ok(s.as_deref() == Some("never"))
    }
}

fn is_not_negated(negated: &bool) -> bool {
    !*negated
}

/// All SpyTial decorators attached to a type or instance.
///
/// Returned by `T::decorators()` for any type that derives [`SpytialDecorators`]
/// (the derive macro), and serialized to YAML by [`to_yaml`] for hand-off to
/// spytial-core.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct SpytialDecorators {
    /// Layout/structural constraints (orientation, alignment, cycles, grouping).
    pub constraints: Vec<Constraint>,
    /// Visual/behavioral directives (color, size, icon, edges, tags, flags, etc.).
    pub directives: Vec<Directive>,
}

impl SpytialDecorators {
    /// Merge `other` in, skipping rules already present.
    ///
    /// Equality ignores `source`: the same rule reached through two types is
    /// one rule, and spytial-core de-duplicates the same way, keeping the
    /// first source it saw. Comparing sources would instead emit the rule
    /// twice, once per site, and the engine would then drop one anyway.
    pub(crate) fn extend_unique(&mut self, other: Self) {
        for constraint in other.constraints {
            let bare = constraint.without_source();
            if !self.constraints.iter().any(|c| c.without_source() == bare) {
                self.constraints.push(constraint);
            }
        }
        for directive in other.directives {
            let bare = directive.without_source();
            if !self.directives.iter().any(|d| d.without_source() == bare) {
                self.directives.push(directive);
            }
        }
    }
}

/// Where a rule was written, as spytial-core's `source` block (5.4).
///
/// The derive stamps every rule it emits with the attribute exactly as the
/// user wrote it and its `file:line`. The viewer's conflict reports and
/// warnings then cite that text in place of the engine's own description of
/// the rule, so a report reads `#[orientation(selector = "left", …)] at
/// src/tree.rs:12` rather than a reconstructed YAML fragment. The manifest
/// accepts the block on every block-bodied form and displays it for the
/// layout constraints and `hideAtom`; on the rest it is carried and ignored.
///
/// Hand-written YAML needs no `source` — there the YAML is the author's text.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuleSource {
    /// The rule as its author wrote it in the host surface.
    pub text: String,
    /// Where it was written, e.g. `src/tree.rs:12`. Appended to `text` in reports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
}

/// A layout/structural constraint on the diagram.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Constraint {
    /// Force one set of atoms to sit above/below/left/right of another.
    Orientation(OrientationConstraint),
    /// Align atoms along the horizontal or vertical axis.
    Align(AlignConstraint),
    /// Lay out atoms in a clockwise or counter-clockwise cycle.
    Cyclic(CyclicConstraint),
    /// Cluster atoms into named groups chosen by a selector.
    Group(GroupConstraint),
    /// Set explicit atom dimensions.
    Size(SizeConstraint),
    /// Hide atoms matching a selector.
    HideAtom(HideAtomConstraint),
}

/// A visual or behavioral directive applied to atoms/relations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Directive {
    /// Style atoms (border, fill, label).
    AtomStyle(AtomStyleDirective),
    /// Style edges (line, label, visibility).
    EdgeStyle(EdgeStyleDirective),
    /// Promote a relation to an inline attribute label on its source atom.
    Attribute(AttributeDirective),
    /// Hide a field/relation from the diagram entirely.
    HideField(HideFieldDirective),
    /// Add a synthesized edge derived from a selector.
    InferredEdge(InferredEdgeDirective),
    /// Tag atoms with a computed attribute value.
    Tag(TagDirective),
    /// Boolean flag (e.g. `hideDisconnected`) toggled on the diagram.
    Flag(FlagDirective),
}

impl Constraint {
    /// The rule's `source` slot. Every constraint form has one.
    pub fn source_mut(&mut self) -> &mut Option<RuleSource> {
        match self {
            Constraint::Orientation(c) => &mut c.orientation.source,
            Constraint::Align(c) => &mut c.align.source,
            Constraint::Cyclic(c) => &mut c.cyclic.source,
            Constraint::Group(c) => match &mut c.group {
                GroupParams::SelectorBased { source, .. } => source,
            },
            Constraint::Size(c) => &mut c.size.source,
            Constraint::HideAtom(c) => &mut c.hide_atom.source,
        }
    }

    /// Where the rule was written, if it was stamped.
    pub fn source(&self) -> Option<&RuleSource> {
        match self {
            Constraint::Orientation(c) => c.orientation.source.as_ref(),
            Constraint::Align(c) => c.align.source.as_ref(),
            Constraint::Cyclic(c) => c.cyclic.source.as_ref(),
            Constraint::Group(c) => match &c.group {
                GroupParams::SelectorBased { source, .. } => source.as_ref(),
            },
            Constraint::Size(c) => c.size.source.as_ref(),
            Constraint::HideAtom(c) => c.hide_atom.source.as_ref(),
        }
    }

    /// This rule with its `source` cleared, for comparisons that should not
    /// care where a rule was written.
    pub fn without_source(&self) -> Self {
        let mut bare = self.clone();
        *bare.source_mut() = None;
        bare
    }
}

impl Directive {
    /// The rule's `source` slot, or `None` for `flag`: it is written as a
    /// bare scalar (`- flag: hideDisconnected`) and has no block to carry one.
    pub fn source_mut(&mut self) -> Option<&mut Option<RuleSource>> {
        match self {
            Directive::AtomStyle(d) => Some(&mut d.atom_style.source),
            Directive::EdgeStyle(d) => Some(&mut d.edge_style.source),
            Directive::Attribute(d) => Some(&mut d.attribute.source),
            Directive::HideField(d) => Some(&mut d.hide_field.source),
            Directive::InferredEdge(d) => Some(&mut d.inferred_edge.source),
            Directive::Tag(d) => Some(&mut d.tag.source),
            Directive::Flag(_) => None,
        }
    }

    /// Where the rule was written, if it was stamped.
    pub fn source(&self) -> Option<&RuleSource> {
        match self {
            Directive::AtomStyle(d) => d.atom_style.source.as_ref(),
            Directive::EdgeStyle(d) => d.edge_style.source.as_ref(),
            Directive::Attribute(d) => d.attribute.source.as_ref(),
            Directive::HideField(d) => d.hide_field.source.as_ref(),
            Directive::InferredEdge(d) => d.inferred_edge.source.as_ref(),
            Directive::Tag(d) => d.tag.source.as_ref(),
            Directive::Flag(_) => None,
        }
    }

    /// This rule with its `source` cleared, for comparisons that should not
    /// care where a rule was written.
    pub fn without_source(&self) -> Self {
        let mut bare = self.clone();
        if let Some(slot) = bare.source_mut() {
            *slot = None;
        }
        bare
    }
}

// Constraint implementations

/// Wire-format wrapper for an `orientation:` constraint.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OrientationConstraint {
    /// Inner orientation parameters (selector, directions, negation).
    pub orientation: OrientationParams,
}

/// Parameters of an [`OrientationConstraint`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OrientationParams {
    /// Selector identifying the atoms the constraint applies to.
    pub selector: String,
    /// Direction tokens (e.g. `"above"`, `"below"`, `"left"`, `"right"`).
    pub directions: Vec<String>,
    /// When `true`, serialized as `hold: never` (i.e. the constraint must NOT hold).
    #[serde(
        rename = "hold",
        default,
        skip_serializing_if = "is_not_negated",
        serialize_with = "hold_serde::serialize",
        deserialize_with = "hold_serde::deserialize"
    )]
    pub negated: bool,
    /// Where this rule came from, for the viewer's conflict reports (spytial-core 5.4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<RuleSource>,
}

/// Wire-format wrapper for an `align:` constraint.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AlignConstraint {
    /// Inner align parameters (selector, direction, negation).
    pub align: AlignParams,
}

/// Parameters of an [`AlignConstraint`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AlignParams {
    /// Selector identifying the atoms the constraint applies to.
    pub selector: String,
    /// Axis to align along (`"horizontal"` or `"vertical"`).
    pub direction: String,
    /// When `true`, serialized as `hold: never`.
    #[serde(
        rename = "hold",
        default,
        skip_serializing_if = "is_not_negated",
        serialize_with = "hold_serde::serialize",
        deserialize_with = "hold_serde::deserialize"
    )]
    pub negated: bool,
    /// Where this rule came from, for the viewer's conflict reports (spytial-core 5.4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<RuleSource>,
}

/// Wire-format wrapper for a `cyclic:` constraint.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CyclicConstraint {
    /// Inner cyclic parameters (selector, direction, negation).
    pub cyclic: CyclicParams,
}

/// Parameters of a [`CyclicConstraint`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CyclicParams {
    /// Selector identifying the relation that defines the cycle.
    pub selector: String,
    /// Cycle direction (`"clockwise"` or `"counterclockwise"`, or an axis token).
    pub direction: String,
    /// When `true`, serialized as `hold: never`.
    #[serde(
        rename = "hold",
        default,
        skip_serializing_if = "is_not_negated",
        serialize_with = "hold_serde::serialize",
        deserialize_with = "hold_serde::deserialize"
    )]
    pub negated: bool,
    /// Where this rule came from, for the viewer's conflict reports (spytial-core 5.4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<RuleSource>,
}

/// Wire-format wrapper for a `group:` constraint.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GroupConstraint {
    /// Inner group parameters — see [`GroupParams`].
    pub group: GroupParams,
}

/// Parameters of a [`GroupConstraint`].
///
/// Kept as an enum so a second shape can be added without a breaking change;
/// [`GroupParams::SelectorBased`] is the only one the language has today.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum GroupParams {
    /// Group atoms matched by a selector. A binary selector builds one group
    /// per distinct first-column atom (the key); a unary one builds a single
    /// unkeyed group.
    ///
    /// This is the only shape. The field-based one (`field`/`groupOn`/
    /// `addToGroup`) was removed from the layout-spec language in spytial-core
    /// 5.1.0 and no longer parses there, so it is not representable here.
    SelectorBased {
        /// Selector identifying the atoms to cluster.
        selector: String,
        /// Group name shown on the diagram.
        name: String,
        /// Connector between the group's key and the group: a bare direction
        /// (`none`/`togroup`/`fromgroup`) or a styled [`GroupEdge`] block
        /// (spytial-core 3.0).
        #[serde(rename = "addEdge", default, skip_serializing_if = "Option::is_none")]
        add_edge: Option<GroupEdgeValue>,
        /// Whether the group caption and its background pill are drawn.
        #[serde(rename = "showLabel", default, skip_serializing_if = "Option::is_none")]
        show_label: Option<bool>,
        /// Styling for the group's own label (spytial-core 3.0; `color` only —
        /// group labels auto-fit their box, so `size` is reserved).
        #[serde(rename = "textStyle", default, skip_serializing_if = "Option::is_none")]
        text_style: Option<TextStyle>,
        /// When `true`, serialized as `hold: never`.
        #[serde(
            rename = "hold",
            default,
            skip_serializing_if = "is_not_negated",
            serialize_with = "hold_serde::serialize",
            deserialize_with = "hold_serde::deserialize"
        )]
        negated: bool,
        /// Where this rule came from, for the viewer's conflict reports (spytial-core 5.4).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<RuleSource>,
    },
}

// Style blocks (spytial-core 3.x style system)
//
// spytial-core 3.0 combined the flat edge/atom styling directives into
// `edgeStyle` / `atomStyle` with nested style blocks, shared by
// `inferredEdge`, `attribute`, `tag` (3.1), and a selector-group's `addEdge`
// connector. The types below are the Rust vocabulary for those YAML blocks;
// enums make the closed vocabularies (pattern, size, points) compile-time
// checked, which matters because spytial-core silently drops invalid leaves.

/// Line dash pattern of a drawn edge (`lineStyle.pattern`).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LinePattern {
    /// A solid line (the default look).
    Solid,
    /// A dashed line.
    Dashed,
    /// A dotted line.
    Dotted,
}

/// Text-size tier of a label (`textStyle.size`), relative to the node label.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TextSize {
    /// Smaller than the node label.
    Small,
    /// Same size as the node label (the default).
    Normal,
    /// Larger than the node label.
    Large,
}

/// Direction of the connector between a selector-group's key and the group
/// (`addEdge` / `addEdge.points`).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum GroupEdgePoints {
    /// Draw no connector (the default).
    None,
    /// Edge from the key into the group.
    Togroup,
    /// Edge from the group back to the key.
    Fromgroup,
}

/// Sparse styling of an edge's drawn line (`lineStyle` block).
///
/// Every field is optional — set only what you mean; absent leaves fall back
/// to the default look. `weight` must be greater than 0 (spytial-core drops
/// non-positive weights).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct LineStyle {
    /// Line colour (any CSS colour string).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Dash pattern.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pattern: Option<LinePattern>,
    /// Line thickness (> 0).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weight: Option<f64>,
    /// Halo/highlight colour drawn behind the line.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub highlight: Option<String>,
}

/// Sparse styling of a label (`textStyle` block) — edge labels, atom labels,
/// attribute/tag lines, group labels.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct TextStyle {
    /// Font-size tier relative to the node label.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<TextSize>,
    /// Text colour (any CSS colour string).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

/// Sparse styling of an atom's border (`borderStyle` block).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct BorderStyle {
    /// Border colour (any CSS colour string).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Border width (> 0).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<f64>,
}

/// Sparse styling of an atom's interior fill (`fillStyle` block).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct FillStyle {
    /// Fill colour (any CSS colour string).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

/// Where an atom's icon sits (`iconStyle.placement`).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum IconPlacement {
    /// The icon occupies the box, which stays transparent unless a
    /// `fillStyle.color` is given. The default.
    Full,
    /// A small marker in the top-right corner, secondary to the label.
    Badge,
}

/// Sparse styling of an atom's icon (`iconStyle` block, spytial-core 4.2).
///
/// Every leaf is optional, `path` included, so a supertype rule can supply the
/// icon and a subtype rule tune only its opacity. An `iconStyle` with no path
/// draws nothing.
///
/// This block plus [`AtomStyleParams::show_label`] replace the old `icon`
/// directive, whose single `showLabels` boolean drove label visibility and icon
/// geometry at once. Splitting them is what makes a faded watermark — or a
/// hidden label with no icon — expressible.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct IconStyle {
    /// A bundled icon name (`person`), an icon-pack reference
    /// (`bi:person-fill`), a URL, or a path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Where the icon sits relative to the label.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placement: Option<IconPlacement>,
    /// Alpha in `[0, 1]`. Out-of-range values are dropped rather than clamped.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f64>,
}

/// Block form of a selector-group's `addEdge`: the connector's direction
/// (`points` — the same key the YAML uses) plus its line and label styling.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GroupEdge {
    /// Which way the connector points.
    pub points: GroupEdgePoints,
    /// Styling for the connector's drawn line.
    #[serde(rename = "lineStyle", skip_serializing_if = "Option::is_none")]
    pub line_style: Option<LineStyle>,
    /// Styling for the connector's label.
    #[serde(rename = "textStyle", skip_serializing_if = "Option::is_none")]
    pub text_style: Option<TextStyle>,
}

/// A selector-group's `addEdge` value: either the bare direction string or
/// the styled block form.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum GroupEdgeValue {
    /// Bare direction form: `addEdge: togroup`.
    Direction(GroupEdgePoints),
    /// Block form: `addEdge: {points: ..., lineStyle: ..., textStyle: ...}`.
    Block(GroupEdge),
}

/// Normalize a legacy `style` string into a [`LinePattern`], the way
/// spytial-core's `normalizeEdgeStyle` does: trim + lowercase, and drop (with
/// a note on stderr) anything outside the solid/dashed/dotted vocabulary —
/// a 2.x-era spec that rendered must keep rendering, not start failing.
pub(crate) fn normalize_legacy_pattern(style: &str) -> Option<LinePattern> {
    match style.trim().to_ascii_lowercase().as_str() {
        "solid" => Some(LinePattern::Solid),
        "dashed" => Some(LinePattern::Dashed),
        "dotted" => Some(LinePattern::Dotted),
        _ => {
            eprintln!(
                "spytial: ignoring invalid edge style {style:?} (expected solid/dashed/dotted); the edge falls back to the default pattern"
            );
            None
        }
    }
}

// Directive implementations

/// Wire-format wrapper for an `atomStyle:` directive (spytial-core 3.0).
///
/// Styles an atom's border, interior fill, and label independently. The
/// legacy `atomColor` authoring forms desugar onto this type with the legacy
/// `value` as the *border* colour (that is what 2.x `atomColor` drew).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AtomStyleDirective {
    /// Inner atom-style parameters.
    #[serde(rename = "atomStyle")]
    pub atom_style: AtomStyleParams,
}

/// Parameters of an [`AtomStyleDirective`]. All fields optional: an absent
/// `selector` matches every atom; absent blocks leave that aspect at the
/// default look.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct AtomStyleParams {
    /// Selector identifying the atoms to style (absent = all atoms).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,
    /// Interior fill styling.
    #[serde(rename = "fillStyle", skip_serializing_if = "Option::is_none")]
    pub fill_style: Option<FillStyle>,
    /// Border styling.
    #[serde(rename = "borderStyle", skip_serializing_if = "Option::is_none")]
    pub border_style: Option<BorderStyle>,
    /// Icon styling (spytial-core 4.2).
    #[serde(rename = "iconStyle", skip_serializing_if = "Option::is_none")]
    pub icon_style: Option<IconStyle>,
    /// Label styling.
    #[serde(rename = "textStyle", skip_serializing_if = "Option::is_none")]
    pub text_style: Option<TextStyle>,
    /// Whether the atom's own label is drawn (spytial-core 4.2; default `true`).
    /// Independent of [`IconStyle::placement`], unlike the `icon` directive's
    /// old `showLabels`.
    #[serde(rename = "showLabel", skip_serializing_if = "Option::is_none")]
    pub show_label: Option<bool>,
    /// Where this rule came from, for the viewer's conflict reports (spytial-core 5.4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<RuleSource>,
}

/// Wire-format wrapper for a `size:` constraint.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SizeConstraint {
    /// Inner size parameters (selector, dimensions).
    pub size: SizeParams,
}

/// Parameters of a [`SizeConstraint`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SizeParams {
    /// Selector identifying the atoms to resize.
    pub selector: String,
    /// Height in diagram units.
    pub height: u32,
    /// Width in diagram units.
    pub width: u32,
    /// Where this rule came from, for the viewer's conflict reports (spytial-core 5.4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<RuleSource>,
}

/// `EdgeStyleDirective` is the canonical edge-styling directive (spytial-core
/// 3.0): the drawn line (`lineStyle`), the edge's label (`textStyle`), and
/// the showLabel/hidden behaviour flags. Mirrors `EdgeStyleRule` in
/// `spytial-core`'s `src/layout/style/edge-style-spec.ts`.
///
/// The legacy flat `edgeColor` authoring forms (`value`/`style`/`weight`)
/// desugar onto this type: `value` -> `lineStyle.color`, `style` ->
/// `lineStyle.pattern`, `weight` -> `lineStyle.weight`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EdgeStyleDirective {
    /// Inner edge-style parameters.
    #[serde(rename = "edgeStyle")]
    pub edge_style: EdgeStyleParams,
}

/// Parameters of an [`EdgeStyleDirective`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EdgeStyleParams {
    /// Relation/field name whose edges this directive styles.
    pub field: String,
    /// Optional selector restricting which edges are styled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,
    /// Optional value filter restricting which edges are styled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,
    /// Styling for the drawn line (colour, pattern, weight, highlight).
    #[serde(rename = "lineStyle", skip_serializing_if = "Option::is_none")]
    pub line_style: Option<LineStyle>,
    /// Styling for the edge's label.
    #[serde(rename = "textStyle", skip_serializing_if = "Option::is_none")]
    pub text_style: Option<TextStyle>,
    /// Whether to render the edge label.
    #[serde(skip_serializing_if = "Option::is_none", rename = "showLabel")]
    pub show_label: Option<bool>,
    /// Whether the edge is hidden entirely.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hidden: Option<bool>,
    /// Where this rule came from, for the viewer's conflict reports (spytial-core 5.4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<RuleSource>,
}

/// Wire-format wrapper for an `attribute:` directive.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AttributeDirective {
    /// Inner attribute parameters.
    pub attribute: AttributeParams,
}

/// Parameters of an [`AttributeDirective`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AttributeParams {
    /// Relation/field name to promote to an inline attribute label.
    pub field: String,
    /// Optional selector restricting where the attribute is shown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,
    /// Optional value filter restricting which tuples are shown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,
    /// Styling for this attribute's line on the node (spytial-core 3.1).
    #[serde(rename = "textStyle", skip_serializing_if = "Option::is_none")]
    pub text_style: Option<TextStyle>,
    /// Where this rule came from, for the viewer's conflict reports (spytial-core 5.4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<RuleSource>,
}

/// Wire-format wrapper for a `hideField:` directive.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HideFieldDirective {
    /// Inner hide-field parameters.
    #[serde(rename = "hideField")]
    pub hide_field: HideFieldParams,
}

/// Parameters of a [`HideFieldDirective`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HideFieldParams {
    /// Relation/field name to hide.
    pub field: String,
    /// Optional selector restricting where the field is hidden.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,
    /// Optional value filter restricting which tuples are hidden.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,
    /// Where this rule came from, for the viewer's conflict reports (spytial-core 5.4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<RuleSource>,
}

/// Wire-format wrapper for a `hideAtom:` constraint.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HideAtomConstraint {
    /// Inner hide-atom parameters.
    #[serde(rename = "hideAtom")]
    pub hide_atom: HideAtomParams,
}

/// Parameters of a [`HideAtomConstraint`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HideAtomParams {
    /// Selector identifying the atoms to hide.
    pub selector: String,
    /// Where this rule came from, for the viewer's conflict reports (spytial-core 5.4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<RuleSource>,
}

/// One end of an `inferredEdge`'s `draw` line: what that end of the edge
/// attaches to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DrawEnd {
    /// Written `_`: the tuple's own atom — what an edge without `draw` does.
    Atom,
    /// A `group` constraint's name: the end attaches to that group's hull.
    /// A keyed (binary-selector) group constraint builds one group per key and
    /// this end's atom picks which; a unary one builds a single group and the
    /// end attaches to it directly.
    Group(String),
}

impl DrawEnd {
    /// The YAML spelling of this end.
    fn as_yaml(&self) -> &str {
        match self {
            DrawEnd::Atom => "_",
            DrawEnd::Group(name) => name,
        }
    }
}

/// An `inferredEdge`'s optional `draw: <end> -> <end>` line (spytial-core 3.2).
///
/// `draw` reinterprets where each end of the edge *attaches*; it never changes
/// which pairs get edges or which way they point (transpose the selector to
/// flip one). The left end applies to each tuple's first atom, the right end to
/// its last. With `draw`, a unary selector is also allowed — the single atom
/// feeds both ends.
///
/// Serializes as the scalar string spytial-core parses, e.g. `_ -> regions`.
/// Group names are resolved against the spec's `group` constraints when
/// spytial-core parses the spec, so a name that no constraint defines is caught
/// there rather than here — decorators compose across types, and no single
/// attribute site can see the whole spec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InferredEdgeDraw {
    /// What the first atom of each tuple attaches to.
    pub source: DrawEnd,
    /// What the last atom of each tuple attaches to.
    pub target: DrawEnd,
}

impl InferredEdgeDraw {
    /// Build a `draw` value from its two ends.
    pub fn new(source: DrawEnd, target: DrawEnd) -> Self {
        Self { source, target }
    }

    /// Parse the YAML scalar form, `<end> -> <end>`, each end `_` or a group
    /// name. Mirrors spytial-core's `parseInferredEdgeDraw`, minus its
    /// `_ -> _` special case: that round-trips here as an explicit pair of atom
    /// ends (spytial-core drops it, so it renders as a plain edge either way).
    pub fn parse(raw: &str) -> Result<Self, String> {
        let parts: Vec<&str> = raw.split("->").collect();
        if parts.len() != 2 {
            return Err(format!(
                "draw must contain exactly one '->' (e.g. \"regions -> regions\" or \"_ -> regions\"); got {raw:?}"
            ));
        }
        let end = |part: &str| -> Result<DrawEnd, String> {
            match part.trim() {
                "" => Err(format!(
                    "draw has an empty endpoint in {raw:?}; each end must be \"_\" or a group name"
                )),
                "_" => Ok(DrawEnd::Atom),
                name => Ok(DrawEnd::Group(name.to_string())),
            }
        };
        Ok(Self::new(end(parts[0])?, end(parts[1])?))
    }
}

impl Serialize for InferredEdgeDraw {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(&format!(
            "{} -> {}",
            self.source.as_yaml(),
            self.target.as_yaml()
        ))
    }
}

impl<'de> Deserialize<'de> for InferredEdgeDraw {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

/// Wire-format wrapper for an `inferredEdge:` directive.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InferredEdgeDirective {
    /// Inner inferred-edge parameters.
    #[serde(rename = "inferredEdge")]
    pub inferred_edge: InferredEdgeParams,
}

/// Parameters of an [`InferredEdgeDirective`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InferredEdgeParams {
    /// Name shown on the synthesized edge.
    pub name: String,
    /// Selector defining which atom pairs are connected.
    pub selector: String,
    /// Where each end of the edge attaches (spytial-core 3.2). Absent means
    /// both ends attach to the tuple's own atoms.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub draw: Option<InferredEdgeDraw>,
    /// Styling for the synthesized edge's drawn line (spytial-core 3.0).
    #[serde(rename = "lineStyle", skip_serializing_if = "Option::is_none")]
    pub line_style: Option<LineStyle>,
    /// Styling for the synthesized edge's label.
    #[serde(rename = "textStyle", skip_serializing_if = "Option::is_none")]
    pub text_style: Option<TextStyle>,
    /// Where this rule came from, for the viewer's conflict reports (spytial-core 5.4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<RuleSource>,
}

/// `TagDirective` adds computed attributes to nodes based on n-ary selector
/// evaluation. Mirrors `TagDirective` in `spytial-core`'s
/// `src/layout/layoutspec.ts` — the canonical YAML form is:
///
/// ```yaml
/// directives:
///   - tag:
///       toTag: 'Person'
///       name: 'status'
///       value: 'Person.status'
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TagDirective {
    /// Inner tag parameters.
    pub tag: TagParams,
}

/// Parameters of a [`TagDirective`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TagParams {
    /// Selector identifying which atoms receive the tag.
    #[serde(rename = "toTag")]
    pub to_tag: String,
    /// Attribute name to display on the tagged atoms.
    pub name: String,
    /// Expression (n-ary selector) evaluated to produce the attribute value.
    pub value: String,
    /// Styling for this tag's line on the node (spytial-core 3.1).
    #[serde(rename = "textStyle", skip_serializing_if = "Option::is_none")]
    pub text_style: Option<TextStyle>,
    /// Where this rule came from, for the viewer's conflict reports (spytial-core 5.4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<RuleSource>,
}

/// A boolean diagram flag (e.g. `hideDisconnected`, `hideEmptyRelations`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FlagDirective {
    /// Flag name to enable.
    pub flag: String,
}

/// Trait implemented by structs with SpyTial decorators.
///
/// Types that `#[derive(SpytialDecorators)]` get an implementation of this
/// trait whose `decorators()` method returns the full set of constraints and
/// directives declared via attributes on the type and its nested field types.
pub trait HasSpytialDecorators {
    /// Return the full decorator set for this type, including decorators
    /// transitively collected from field types at compile time.
    fn decorators() -> SpytialDecorators;
}

/// One derive-generated entry in the link-time decorator registry.
///
/// This is public only so [`SpytialDecorators`](spytial_export_macros::SpytialDecorators)
/// expansions in downstream crates can submit entries. Applications should
/// not construct registrations directly.
#[doc(hidden)]
pub struct DecoratorRegistration {
    rust_type_name: fn() -> &'static str,
    rust_type_name_is_generic: bool,
    rust_short_type_name: &'static str,
    serialized_type_name: &'static str,
    decorators: fn() -> SpytialDecorators,
}

impl DecoratorRegistration {
    /// Construct a derive-generated registration.
    #[doc(hidden)]
    pub const fn new(
        rust_type_name: fn() -> &'static str,
        rust_type_name_is_generic: bool,
        rust_short_type_name: &'static str,
        serialized_type_name: &'static str,
        decorators: fn() -> SpytialDecorators,
    ) -> Self {
        Self {
            rust_type_name,
            rust_type_name_is_generic,
            rust_short_type_name,
            serialized_type_name,
            decorators,
        }
    }

    fn matches_root_type(&self, root_type_name: &str) -> bool {
        let registered_name = (self.rust_type_name)();
        root_type_name == registered_name
            || (self.rust_type_name_is_generic
                && root_type_name
                    .strip_prefix(registered_name)
                    .is_some_and(|suffix| suffix.starts_with('<')))
    }

    fn matches_serialized_type(&self, type_name: &str) -> bool {
        self.rust_short_type_name == type_name || self.serialized_type_name == type_name
    }
}

inventory::collect!(DecoratorRegistration);

impl<T: HasSpytialDecorators + ?Sized> HasSpytialDecorators for &T {
    fn decorators() -> SpytialDecorators {
        T::decorators()
    }
}

// Probe mechanism for safe compile-time decorator collection.
//
// The derive macro collects decorators from field types, but at expansion time
// it can't tell whether a given field type implements `HasSpytialDecorators` —
// emitting a call that requires the bound would fail to compile for types that
// don't. The probe defers the decision to the call site, where the concrete type
// is known. Inherent methods outrank trait methods in Rust's method resolution,
// so `DecoProbe::<T>::get` resolves to the inherent method (real `T::decorators()`)
// when `T: HasSpytialDecorators`, and to the blanket `DefaultDecorators::get`
// (an empty set) otherwise.

/// Zero-sized probe used by macro-generated code to safely collect
/// decorators from a type that may or may not implement
/// [`HasSpytialDecorators`].
pub struct DecoProbe<T>(
    /// Ties the probe to its type parameter `T`.
    pub ::std::marker::PhantomData<T>,
);

/// Inherent impl – available only when `T` has the derive.
/// Because inherent methods take priority over trait methods, this is
/// chosen whenever it exists.
impl<T: HasSpytialDecorators> DecoProbe<T> {
    /// Real path: forwards to `T::decorators()`.
    pub fn get(self) -> SpytialDecorators {
        T::decorators()
    }
}

/// Blanket fallback – available for *every* `T`.  Chosen only when the
/// inherent `get` above does not exist (i.e. `T` does not implement
/// `HasSpytialDecorators`).
pub trait DefaultDecorators {
    /// Fallback path: returns [`SpytialDecorators::default()`].
    fn get(self) -> SpytialDecorators;
}

impl<T> DefaultDecorators for DecoProbe<T> {
    fn get(self) -> SpytialDecorators {
        SpytialDecorators::default()
    }
}

/// Global registry for type-level decorators keyed by type name
static TYPE_REGISTRY: LazyLock<Mutex<HashMap<String, SpytialDecorators>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Register SpyTial decorators for a type, keyed by name.
///
/// Called by code generated from `#[derive(SpytialDecorators)]` the first
/// time `T::decorators()` is invoked. End users normally do not call this
/// directly.
pub fn register_type_decorators(type_name: &str, decorators: SpytialDecorators) {
    // Recover from a poisoned lock: a panic in a previous decorator builder
    // should not permanently brick decorator collection for the rest of the
    // process. The data behind the lock is just a HashMap; partial writes are
    // not a soundness issue here.
    let mut registry = TYPE_REGISTRY
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    registry.insert(type_name.to_string(), decorators);
}

/// Look up explicitly registered decorators for `type_name`, if any.
///
/// Returns `None` if the type has never had its `decorators()` method called
/// (and therefore never registered itself with the global registry), and no
/// caller has registered the name manually.
pub fn get_type_decorators(type_name: &str) -> Option<SpytialDecorators> {
    TYPE_REGISTRY
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(type_name)
        .cloned()
}

enum RegistrationMatch {
    None,
    Unique(&'static DecoratorRegistration),
    Ambiguous,
}

fn unique_registration(
    mut matches: impl Iterator<Item = &'static DecoratorRegistration>,
) -> RegistrationMatch {
    let Some(first) = matches.next() else {
        return RegistrationMatch::None;
    };
    if matches.next().is_some() {
        RegistrationMatch::Ambiguous
    } else {
        RegistrationMatch::Unique(first)
    }
}

fn serialized_decorators_for_registration(
    registration: &DecoratorRegistration,
) -> SpytialDecorators {
    get_type_decorators((registration.rust_type_name)())
        .or_else(|| get_type_decorators(registration.serialized_type_name))
        .or_else(|| get_type_decorators(registration.rust_short_type_name))
        .unwrap_or_else(|| (registration.decorators)())
}

fn peel_references(mut type_name: &str) -> &str {
    loop {
        let Some(rest) = type_name.strip_prefix('&') else {
            return type_name;
        };
        type_name = rest.trim_start();
        if let Some(rest) = type_name.strip_prefix("mut ") {
            type_name = rest;
        }
    }
}

/// Resolve the concrete root type before Serde has a chance to erase its
/// identity (for example through `#[serde(transparent)]` or `untagged`).
pub(crate) fn get_linked_root_type_decorators(root_type_name: &str) -> Option<SpytialDecorators> {
    let root_type_name = peel_references(root_type_name);
    match unique_registration(
        inventory::iter::<DecoratorRegistration>
            .into_iter()
            .filter(|registration| registration.matches_root_type(root_type_name)),
    ) {
        // Once the qualified root has selected its registration, do not fall
        // back to an unqualified runtime key: another same-named type may have
        // populated it. Only the exact concrete key can override this entry.
        RegistrationMatch::Unique(registration) => {
            Some(get_type_decorators(root_type_name).unwrap_or_else(|| (registration.decorators)()))
        }
        RegistrationMatch::None | RegistrationMatch::Ambiguous => None,
    }
}

/// Resolve the name exposed by a Serde callback. These names are unqualified,
/// so an ambiguous match must contribute nothing rather than silently mixing
/// decorators from unrelated types.
pub(crate) fn get_linked_serialized_type_decorators(type_name: &str) -> Option<SpytialDecorators> {
    match unique_registration(
        inventory::iter::<DecoratorRegistration>
            .into_iter()
            .filter(|registration| registration.matches_serialized_type(type_name)),
    ) {
        RegistrationMatch::None => get_type_decorators(type_name),
        RegistrationMatch::Unique(registration) => {
            Some(serialized_decorators_for_registration(registration))
        }
        RegistrationMatch::Ambiguous => None,
    }
}

/// Serialize a [`SpytialDecorators`] value to its YAML wire format.
pub fn to_yaml(decorators: &SpytialDecorators) -> Result<String, serde_yaml_ng::Error> {
    serde_yaml_ng::to_string(decorators)
}

/// Programmatic builder for [`SpytialDecorators`].
///
/// Usually constructed by code generated from `#[derive(SpytialDecorators)]`,
/// but also useful when assembling a decorator set by hand.
#[derive(Debug)]
pub struct SpytialDecoratorsBuilder {
    constraints: Vec<Constraint>,
    directives: Vec<Directive>,
    /// Which list took the most recent rule, so [`Self::source`] can find it.
    last: Option<LastPushed>,
}

#[derive(Debug, Clone, Copy)]
enum LastPushed {
    Constraint,
    Directive,
}

impl Default for SpytialDecoratorsBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl SpytialDecoratorsBuilder {
    /// Create a new builder with no constraints and no directives.
    pub fn new() -> Self {
        Self {
            constraints: Vec::new(),
            directives: Vec::new(),
            last: None,
        }
    }

    fn push_constraint(&mut self, constraint: Constraint) {
        self.constraints.push(constraint);
        self.last = Some(LastPushed::Constraint);
    }

    fn push_directive(&mut self, directive: Directive) {
        self.directives.push(directive);
        self.last = Some(LastPushed::Directive);
    }

    /// Stamp the most recently pushed rule with where it was written
    /// (spytial-core 5.4's `source` block).
    ///
    /// The derive calls this after every rule it emits, passing the attribute
    /// as the user wrote it and its `file:line`, so the viewer's conflict
    /// reports cite the Rust attribute rather than a reconstructed YAML
    /// fragment. Does nothing when no rule has been pushed yet, when the last
    /// one was a `flag` (a bare scalar with nowhere to carry a `source`), or
    /// directly after [`Self::extend_with`], whose rules keep the sources they
    /// arrived with. Hand-assembled builders can skip it: a rule without a
    /// `source` is reported by spytial-core's own description of it.
    pub fn source(mut self, text: &str, location: Option<&str>) -> Self {
        let source = RuleSource {
            text: text.to_string(),
            location: location.map(str::to_string),
        };
        match self.last {
            Some(LastPushed::Constraint) => {
                if let Some(constraint) = self.constraints.last_mut() {
                    *constraint.source_mut() = Some(source);
                }
            }
            Some(LastPushed::Directive) => {
                if let Some(Some(slot)) = self.directives.last_mut().map(Directive::source_mut) {
                    *slot = Some(source);
                }
            }
            None => {}
        }
        self
    }

    /// Push an [`OrientationConstraint`] onto the builder.
    pub fn orientation(mut self, selector: &str, directions: Vec<&str>, negated: bool) -> Self {
        self.push_constraint(Constraint::Orientation(OrientationConstraint {
            orientation: OrientationParams {
                source: None,
                selector: selector.to_string(),
                directions: directions.iter().map(|s| s.to_string()).collect(),
                negated,
            },
        }));
        self
    }

    /// Push an [`AlignConstraint`] onto the builder.
    pub fn align(mut self, selector: &str, direction: &str, negated: bool) -> Self {
        self.push_constraint(Constraint::Align(AlignConstraint {
            align: AlignParams {
                source: None,
                selector: selector.to_string(),
                direction: direction.to_string(),
                negated,
            },
        }));
        self
    }

    /// Push a [`CyclicConstraint`] onto the builder.
    pub fn cyclic(mut self, selector: &str, direction: &str, negated: bool) -> Self {
        self.push_constraint(Constraint::Cyclic(CyclicConstraint {
            cyclic: CyclicParams {
                source: None,
                selector: selector.to_string(),
                direction: direction.to_string(),
                negated,
            },
        }));
        self
    }

    /// Push a selector-based [`GroupConstraint`] (clusters atoms under a
    /// single named group) onto the builder.
    pub fn group_selector_based(self, selector: &str, name: &str, negated: bool) -> Self {
        self.group_selector_based_styled(selector, name, None, None, negated)
    }

    /// Push a selector-based [`GroupConstraint`] with connector and label
    /// styling (spytial-core 3.0) onto the builder.
    pub fn group_selector_based_styled(
        self,
        selector: &str,
        name: &str,
        add_edge: Option<GroupEdgeValue>,
        text_style: Option<TextStyle>,
        negated: bool,
    ) -> Self {
        self.group_selector_based_styled_with_label(
            selector, name, add_edge, None, text_style, negated,
        )
    }

    /// Push a selector-based [`GroupConstraint`] with explicit caption visibility.
    pub fn group_selector_based_styled_with_label(
        mut self,
        selector: &str,
        name: &str,
        add_edge: Option<GroupEdgeValue>,
        show_label: Option<bool>,
        text_style: Option<TextStyle>,
        negated: bool,
    ) -> Self {
        self.push_constraint(Constraint::Group(GroupConstraint {
            group: GroupParams::SelectorBased {
                source: None,
                selector: selector.to_string(),
                name: name.to_string(),
                add_edge,
                show_label,
                text_style,
                negated,
            },
        }));
        self
    }

    /// Push an [`AtomStyleDirective`] onto the builder (spytial-core 3.0).
    #[allow(clippy::too_many_arguments)]
    pub fn atom_style(
        mut self,
        selector: Option<&str>,
        fill_style: Option<FillStyle>,
        border_style: Option<BorderStyle>,
        icon_style: Option<IconStyle>,
        text_style: Option<TextStyle>,
        show_label: Option<bool>,
    ) -> Self {
        self.push_directive(Directive::AtomStyle(AtomStyleDirective {
            atom_style: AtomStyleParams {
                source: None,
                selector: selector.map(|s| s.to_string()),
                fill_style,
                border_style,
                icon_style,
                text_style,
                show_label,
            },
        }));
        self
    }

    /// Push the legacy flat `atomColor` form onto the builder.
    ///
    /// Rewrites to an [`AtomStyleDirective`] with `value` as the *border*
    /// colour — that is what the 2.x directive drew, so existing specs keep
    /// their look. Prefer [`Self::atom_style`].
    pub fn atom_color(self, selector: &str, value: &str) -> Self {
        self.atom_style(
            Some(selector),
            None,
            Some(BorderStyle {
                color: Some(value.to_string()),
                width: None,
            }),
            None,
            None,
            None,
        )
    }

    /// Push a [`SizeConstraint`] onto the builder.
    pub fn size(mut self, selector: &str, height: u32, width: u32) -> Self {
        self.push_constraint(Constraint::Size(SizeConstraint {
            size: SizeParams {
                source: None,
                selector: selector.to_string(),
                height,
                width,
            },
        }));
        self
    }

    /// Push the legacy `icon` form onto the builder.
    ///
    /// Rewrites to an [`AtomStyleDirective`], which is the mapping spytial-core
    /// 4.2 documents when it deprecated `icon`: the single `showLabels` boolean
    /// drove label visibility and icon geometry at once, so it splits into an
    /// independent [`IconStyle::placement`] and [`AtomStyleParams::show_label`]
    /// — `showLabels: true` becomes a `badge` beside a visible label, and
    /// `showLabels: false` becomes a `full` icon with the label off. Prefer
    /// [`Self::atom_style`].
    pub fn icon(self, selector: &str, path: &str, show_labels: bool) -> Self {
        self.atom_style(
            Some(selector),
            None,
            None,
            Some(IconStyle {
                path: Some(path.to_string()),
                placement: Some(if show_labels {
                    IconPlacement::Badge
                } else {
                    IconPlacement::Full
                }),
                opacity: None,
            }),
            None,
            Some(show_labels),
        )
    }

    /// Push an [`EdgeStyleDirective`] onto the builder (spytial-core 3.0).
    #[allow(clippy::too_many_arguments)]
    pub fn edge_style(
        mut self,
        field: &str,
        selector: Option<&str>,
        filter: Option<&str>,
        line_style: Option<LineStyle>,
        text_style: Option<TextStyle>,
        show_label: Option<bool>,
        hidden: Option<bool>,
    ) -> Self {
        self.push_directive(Directive::EdgeStyle(EdgeStyleDirective {
            edge_style: EdgeStyleParams {
                source: None,
                field: field.to_string(),
                selector: selector.map(|s| s.to_string()),
                filter: filter.map(|s| s.to_string()),
                line_style,
                text_style,
                show_label,
                hidden,
            },
        }));
        self
    }

    /// Push the legacy flat `edgeColor` form onto the builder.
    ///
    /// Rewrites to an [`EdgeStyleDirective`]: `value` -> `lineStyle.color`,
    /// `style` -> `lineStyle.pattern` (normalized like spytial-core: trimmed,
    /// lowercased; an unrecognized pattern is dropped with a note on stderr
    /// rather than failing, so 2.x-era specs keep rendering), `weight` ->
    /// `lineStyle.weight` (non-positive dropped). Prefer [`Self::edge_style`].
    #[allow(clippy::too_many_arguments)]
    pub fn edge_color(
        self,
        field: &str,
        value: &str,
        selector: Option<&str>,
        filter: Option<&str>,
        style: Option<&str>,
        weight: Option<f64>,
        show_label: Option<bool>,
        hidden: Option<bool>,
    ) -> Self {
        let line_style = LineStyle {
            color: Some(value.to_string()),
            pattern: style.and_then(normalize_legacy_pattern),
            weight: weight.filter(|w| {
                let ok = w.is_finite() && *w > 0.0;
                if !ok {
                    eprintln!("spytial: ignoring invalid edge weight {w:?}; the edge falls back to the default thickness");
                }
                ok
            }),
            highlight: None,
        };
        self.edge_style(
            field,
            selector,
            filter,
            Some(line_style),
            None,
            show_label,
            hidden,
        )
    }

    /// Push an [`AttributeDirective`] onto the builder.
    pub fn attribute(self, field: &str, selector: Option<&str>) -> Self {
        self.attribute_styled(field, selector, None, None)
    }

    /// Push an [`AttributeDirective`] with label styling (spytial-core 3.1)
    /// onto the builder.
    pub fn attribute_styled(
        mut self,
        field: &str,
        selector: Option<&str>,
        filter: Option<&str>,
        text_style: Option<TextStyle>,
    ) -> Self {
        self.push_directive(Directive::Attribute(AttributeDirective {
            attribute: AttributeParams {
                source: None,
                field: field.to_string(),
                selector: selector.map(|s| s.to_string()),
                filter: filter.map(|s| s.to_string()),
                text_style,
            },
        }));
        self
    }

    /// Push a [`HideFieldDirective`] onto the builder.
    pub fn hide_field(mut self, field: &str, selector: Option<&str>, filter: Option<&str>) -> Self {
        self.push_directive(Directive::HideField(HideFieldDirective {
            hide_field: HideFieldParams {
                source: None,
                field: field.to_string(),
                selector: selector.map(|s| s.to_string()),
                filter: filter.map(|s| s.to_string()),
            },
        }));
        self
    }

    /// Push a [`HideAtomConstraint`] onto the builder.
    pub fn hide_atom(mut self, selector: &str) -> Self {
        self.push_constraint(Constraint::HideAtom(HideAtomConstraint {
            hide_atom: HideAtomParams {
                source: None,
                selector: selector.to_string(),
            },
        }));
        self
    }

    /// Push an [`InferredEdgeDirective`] onto the builder.
    pub fn inferred_edge(self, name: &str, selector: &str) -> Self {
        self.inferred_edge_styled(name, selector, None, None)
    }

    /// Push an [`InferredEdgeDirective`] with line/label styling
    /// (spytial-core 3.0) onto the builder.
    pub fn inferred_edge_styled(
        self,
        name: &str,
        selector: &str,
        line_style: Option<LineStyle>,
        text_style: Option<TextStyle>,
    ) -> Self {
        self.inferred_edge_drawn(name, selector, None, line_style, text_style)
    }

    /// Push an [`InferredEdgeDirective`] with group-hull endpoints
    /// (spytial-core 3.2) and line/label styling onto the builder.
    pub fn inferred_edge_drawn(
        mut self,
        name: &str,
        selector: &str,
        draw: Option<InferredEdgeDraw>,
        line_style: Option<LineStyle>,
        text_style: Option<TextStyle>,
    ) -> Self {
        self.push_directive(Directive::InferredEdge(InferredEdgeDirective {
            inferred_edge: InferredEdgeParams {
                source: None,
                name: name.to_string(),
                selector: selector.to_string(),
                draw,
                line_style,
                text_style,
            },
        }));
        self
    }

    /// Push a [`FlagDirective`] onto the builder.
    pub fn flag(mut self, name: &str) -> Self {
        self.push_directive(Directive::Flag(FlagDirective {
            flag: name.to_string(),
        }));
        self
    }

    /// Push a [`TagDirective`] onto the builder.
    pub fn tag(self, to_tag: &str, name: &str, value: &str) -> Self {
        self.tag_styled(to_tag, name, value, None)
    }

    /// Push a [`TagDirective`] with label styling (spytial-core 3.1) onto
    /// the builder.
    pub fn tag_styled(
        mut self,
        to_tag: &str,
        name: &str,
        value: &str,
        text_style: Option<TextStyle>,
    ) -> Self {
        self.push_directive(Directive::Tag(TagDirective {
            tag: TagParams {
                source: None,
                to_tag: to_tag.to_string(),
                name: name.to_string(),
                value: value.to_string(),
                text_style,
            },
        }));
        self
    }

    /// Include decorators from another type that implements `HasSpytialDecorators`.
    pub fn include_decorators_from_type<T: HasSpytialDecorators>(self) -> Self {
        self.extend_with(T::decorators())
    }

    /// Merge another set of decorators into this builder.
    ///
    /// Used by the derive macro together with [`DecoProbe`] for safe
    /// compile-time decorator collection from field types that may or may
    /// not implement [`HasSpytialDecorators`].
    pub fn extend_with(mut self, other: SpytialDecorators) -> Self {
        self.constraints.extend(other.constraints);
        self.directives.extend(other.directives);
        // The merged rules keep the sources they arrived with; a `.source()`
        // written after this must not re-stamp the last of them.
        self.last = None;
        self
    }

    /// Consume the builder and return the assembled [`SpytialDecorators`].
    pub fn build(self) -> SpytialDecorators {
        SpytialDecorators {
            constraints: self.constraints,
            directives: self.directives,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spytial_decorators_default() {
        let decorators = SpytialDecorators::default();
        assert!(decorators.constraints.is_empty());
        assert!(decorators.directives.is_empty());
    }

    #[test]
    fn test_yaml_serialization() {
        let decorators = SpytialDecorators {
            constraints: vec![
                Constraint::Orientation(OrientationConstraint {
                    orientation: OrientationParams {
                        source: None,
                        selector: "value".to_string(),
                        directions: vec!["above".to_string()],
                        negated: false,
                    },
                }),
                Constraint::Align(AlignConstraint {
                    align: AlignParams {
                        source: None,
                        selector: "siblings".to_string(),
                        direction: "horizontal".to_string(),
                        negated: false,
                    },
                }),
            ],
            directives: vec![Directive::Flag(FlagDirective {
                flag: "test_flag".to_string(),
            })],
        };

        let yaml = to_yaml(&decorators).unwrap();
        assert!(yaml.contains("orientation"));
        assert!(yaml.contains("align"));
        assert!(yaml.contains("flag"));
    }
}
