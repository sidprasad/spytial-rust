use serde::{Deserialize, Serialize};
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
    /// Cluster atoms into named groups, either by selector or by field.
    Group(GroupConstraint),
}

/// A visual or behavioral directive applied to atoms/relations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Directive {
    /// Set atom fill colour.
    AtomColor(AtomColorDirective),
    /// Set explicit atom dimensions.
    Size(SizeDirective),
    /// Render atoms as an image icon.
    Icon(IconDirective),
    /// Style edges (colour, line style, weight, visibility, label).
    EdgeStyle(EdgeStyleDirective),
    /// Project atoms of a given signature out of the main view.
    Projection(ProjectionDirective),
    /// Promote a relation to an inline attribute label on its source atom.
    Attribute(AttributeDirective),
    /// Hide a field/relation from the diagram entirely.
    HideField(HideFieldDirective),
    /// Hide atoms matching a selector.
    HideAtom(HideAtomDirective),
    /// Add a synthesized edge derived from a selector.
    InferredEdge(InferredEdgeDirective),
    /// Tag atoms with a computed attribute value.
    Tag(TagDirective),
    /// Boolean flag (e.g. `hideDisconnected`) toggled on the diagram.
    Flag(FlagDirective),
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
}

/// Wire-format wrapper for a `group:` constraint.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GroupConstraint {
    /// Inner group parameters (one of two shapes — see [`GroupParams`]).
    pub group: GroupParams,
}

/// Parameters of a [`GroupConstraint`].
///
/// Group constraints come in two flavours: cluster atoms by an explicit
/// selector ([`GroupParams::SelectorBased`]) or by following a relation field
/// ([`GroupParams::FieldBased`]).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum GroupParams {
    /// Group atoms by following a relation field and indices.
    FieldBased {
        /// Name of the relation/field to group on.
        field: String,
        /// Tuple index used to identify the group key.
        #[serde(rename = "groupOn")]
        group_on: u32,
        /// Tuple index whose atom is added to the group.
        #[serde(rename = "addToGroup")]
        add_to_group: u32,
        /// Optional selector restricting which tuples participate.
        #[serde(skip_serializing_if = "Option::is_none")]
        selector: Option<String>,
        /// When `true`, serialized as `hold: never`.
        #[serde(
            rename = "hold",
            default,
            skip_serializing_if = "is_not_negated",
            serialize_with = "hold_serde::serialize",
            deserialize_with = "hold_serde::deserialize"
        )]
        negated: bool,
    },
    /// Group atoms matched by a selector under a single named cluster.
    SelectorBased {
        /// Selector identifying the atoms to cluster.
        selector: String,
        /// Group name shown on the diagram.
        name: String,
        /// When `true`, serialized as `hold: never`.
        #[serde(
            rename = "hold",
            default,
            skip_serializing_if = "is_not_negated",
            serialize_with = "hold_serde::serialize",
            deserialize_with = "hold_serde::deserialize"
        )]
        negated: bool,
    },
}

// Directive implementations

/// Wire-format wrapper for an `atomColor:` directive.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AtomColorDirective {
    /// Inner atom-color parameters (selector, colour value).
    #[serde(rename = "atomColor")]
    pub atom_color: AtomColorParams,
}

/// Parameters of an [`AtomColorDirective`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AtomColorParams {
    /// Selector identifying the atoms to recolour.
    pub selector: String,
    /// CSS-style colour value (e.g. `"red"`, `"#ff0000"`).
    pub value: String,
}

/// Wire-format wrapper for a `size:` directive.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SizeDirective {
    /// Inner size parameters (selector, dimensions).
    pub size: SizeParams,
}

/// Parameters of a [`SizeDirective`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SizeParams {
    /// Selector identifying the atoms to resize.
    pub selector: String,
    /// Height in diagram units.
    pub height: u32,
    /// Width in diagram units.
    pub width: u32,
}

/// Wire-format wrapper for an `icon:` directive.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IconDirective {
    /// Inner icon parameters (selector, image path, label flag).
    pub icon: IconParams,
}

/// Parameters of an [`IconDirective`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IconParams {
    /// Selector identifying the atoms to render as icons.
    pub selector: String,
    /// Path or URL to the icon image.
    pub path: String,
    /// Whether to keep the atom label visible alongside the icon.
    #[serde(rename = "showLabels")]
    pub show_labels: bool,
}

/// `EdgeStyleDirective` is the canonical edge-styling directive — color,
/// line style, weight, label visibility, and edge visibility in one
/// directive. Mirrors `EdgeStyleDirective` in `spytial-core`'s
/// `src/layout/layoutspec.ts`.
///
/// The wire-format YAML key is `edgeColor:` (kept for backwards
/// compatibility with `spytial-core`'s parser, where `EdgeColorDirective`
/// is a type alias for `EdgeStyleDirective`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EdgeStyleDirective {
    /// Inner edge-style parameters.
    #[serde(rename = "edgeColor")]
    pub edge_style: EdgeStyleParams,
}

/// Parameters of an [`EdgeStyleDirective`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EdgeStyleParams {
    /// Relation/field name whose edges this directive styles.
    pub field: String,
    /// Edge colour (CSS-style value).
    pub value: String,
    /// Optional selector restricting which edges are styled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,
    /// Optional value filter restricting which edges are styled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,
    /// Optional line style (e.g. `"solid"`, `"dashed"`, `"dotted"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
    /// Optional line weight (thickness).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weight: Option<f64>,
    /// Whether to render the edge label.
    #[serde(skip_serializing_if = "Option::is_none", rename = "showLabel")]
    pub show_label: Option<bool>,
    /// Whether the edge is hidden entirely.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hidden: Option<bool>,
}

/// Wire-format wrapper for a `projection:` directive.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectionDirective {
    /// Inner projection parameters.
    pub projection: ProjectionParams,
}

/// Parameters of a [`ProjectionDirective`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectionParams {
    /// Signature/type name to project out of the main view.
    pub sig: String,
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
}

/// Wire-format wrapper for a `hideAtom:` directive.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HideAtomDirective {
    /// Inner hide-atom parameters.
    #[serde(rename = "hideAtom")]
    pub hide_atom: HideAtomParams,
}

/// Parameters of a [`HideAtomDirective`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HideAtomParams {
    /// Selector identifying the atoms to hide.
    pub selector: String,
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

/// Look up previously-registered decorators for `type_name`, if any.
///
/// Returns `None` if the type has never had its `decorators()` method called
/// (and therefore never registered itself with the global registry).
pub fn get_type_decorators(type_name: &str) -> Option<SpytialDecorators> {
    let registry = TYPE_REGISTRY
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    registry.get(type_name).cloned()
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
        }
    }

    /// Push an [`OrientationConstraint`] onto the builder.
    pub fn orientation(mut self, selector: &str, directions: Vec<&str>, negated: bool) -> Self {
        self.constraints
            .push(Constraint::Orientation(OrientationConstraint {
                orientation: OrientationParams {
                    selector: selector.to_string(),
                    directions: directions.iter().map(|s| s.to_string()).collect(),
                    negated,
                },
            }));
        self
    }

    /// Push an [`AlignConstraint`] onto the builder.
    pub fn align(mut self, selector: &str, direction: &str, negated: bool) -> Self {
        self.constraints.push(Constraint::Align(AlignConstraint {
            align: AlignParams {
                selector: selector.to_string(),
                direction: direction.to_string(),
                negated,
            },
        }));
        self
    }

    /// Push a [`CyclicConstraint`] onto the builder.
    pub fn cyclic(mut self, selector: &str, direction: &str, negated: bool) -> Self {
        self.constraints.push(Constraint::Cyclic(CyclicConstraint {
            cyclic: CyclicParams {
                selector: selector.to_string(),
                direction: direction.to_string(),
                negated,
            },
        }));
        self
    }

    /// Push a field-based [`GroupConstraint`] (groups by following a relation
    /// field) onto the builder.
    pub fn group_field_based(
        mut self,
        field: &str,
        group_on: u32,
        add_to_group: u32,
        selector: Option<&str>,
        negated: bool,
    ) -> Self {
        self.constraints.push(Constraint::Group(GroupConstraint {
            group: GroupParams::FieldBased {
                field: field.to_string(),
                group_on,
                add_to_group,
                selector: selector.map(|s| s.to_string()),
                negated,
            },
        }));
        self
    }

    /// Push a selector-based [`GroupConstraint`] (clusters atoms under a
    /// single named group) onto the builder.
    pub fn group_selector_based(mut self, selector: &str, name: &str, negated: bool) -> Self {
        self.constraints.push(Constraint::Group(GroupConstraint {
            group: GroupParams::SelectorBased {
                selector: selector.to_string(),
                name: name.to_string(),
                negated,
            },
        }));
        self
    }

    /// Push an [`AtomColorDirective`] onto the builder.
    pub fn atom_color(mut self, selector: &str, value: &str) -> Self {
        self.directives
            .push(Directive::AtomColor(AtomColorDirective {
                atom_color: AtomColorParams {
                    selector: selector.to_string(),
                    value: value.to_string(),
                },
            }));
        self
    }

    /// Push a [`SizeDirective`] onto the builder.
    pub fn size(mut self, selector: &str, height: u32, width: u32) -> Self {
        self.directives.push(Directive::Size(SizeDirective {
            size: SizeParams {
                selector: selector.to_string(),
                height,
                width,
            },
        }));
        self
    }

    /// Push an [`IconDirective`] onto the builder.
    pub fn icon(mut self, selector: &str, path: &str, show_labels: bool) -> Self {
        self.directives.push(Directive::Icon(IconDirective {
            icon: IconParams {
                selector: selector.to_string(),
                path: path.to_string(),
                show_labels,
            },
        }));
        self
    }

    /// Push an [`EdgeStyleDirective`] onto the builder.
    #[allow(clippy::too_many_arguments)]
    pub fn edge_style(
        mut self,
        field: &str,
        value: &str,
        selector: Option<&str>,
        filter: Option<&str>,
        style: Option<&str>,
        weight: Option<f64>,
        show_label: Option<bool>,
        hidden: Option<bool>,
    ) -> Self {
        self.directives
            .push(Directive::EdgeStyle(EdgeStyleDirective {
                edge_style: EdgeStyleParams {
                    field: field.to_string(),
                    value: value.to_string(),
                    selector: selector.map(|s| s.to_string()),
                    filter: filter.map(|s| s.to_string()),
                    style: style.map(|s| s.to_string()),
                    weight,
                    show_label,
                    hidden,
                },
            }));
        self
    }

    /// Push a [`ProjectionDirective`] onto the builder.
    pub fn projection(mut self, sig: &str) -> Self {
        self.directives
            .push(Directive::Projection(ProjectionDirective {
                projection: ProjectionParams {
                    sig: sig.to_string(),
                },
            }));
        self
    }

    /// Push an [`AttributeDirective`] onto the builder.
    pub fn attribute(mut self, field: &str, selector: Option<&str>) -> Self {
        self.directives
            .push(Directive::Attribute(AttributeDirective {
                attribute: AttributeParams {
                    field: field.to_string(),
                    selector: selector.map(|s| s.to_string()),
                },
            }));
        self
    }

    /// Push a [`HideFieldDirective`] onto the builder.
    pub fn hide_field(mut self, field: &str, selector: Option<&str>) -> Self {
        self.directives
            .push(Directive::HideField(HideFieldDirective {
                hide_field: HideFieldParams {
                    field: field.to_string(),
                    selector: selector.map(|s| s.to_string()),
                },
            }));
        self
    }

    /// Push a [`HideAtomDirective`] onto the builder.
    pub fn hide_atom(mut self, selector: &str) -> Self {
        self.directives.push(Directive::HideAtom(HideAtomDirective {
            hide_atom: HideAtomParams {
                selector: selector.to_string(),
            },
        }));
        self
    }

    /// Push an [`InferredEdgeDirective`] onto the builder.
    pub fn inferred_edge(mut self, name: &str, selector: &str) -> Self {
        self.directives
            .push(Directive::InferredEdge(InferredEdgeDirective {
                inferred_edge: InferredEdgeParams {
                    name: name.to_string(),
                    selector: selector.to_string(),
                },
            }));
        self
    }

    /// Push a [`FlagDirective`] onto the builder.
    pub fn flag(mut self, name: &str) -> Self {
        self.directives.push(Directive::Flag(FlagDirective {
            flag: name.to_string(),
        }));
        self
    }

    /// Push a [`TagDirective`] onto the builder.
    pub fn tag(mut self, to_tag: &str, name: &str, value: &str) -> Self {
        self.directives.push(Directive::Tag(TagDirective {
            tag: TagParams {
                to_tag: to_tag.to_string(),
                name: name.to_string(),
                value: value.to_string(),
            },
        }));
        self
    }

    /// Include decorators from another type that implements `HasSpytialDecorators`.
    pub fn include_decorators_from_type<T: HasSpytialDecorators>(mut self) -> Self {
        let other_decorators = T::decorators();
        self.constraints.extend(other_decorators.constraints);
        self.directives.extend(other_decorators.directives);
        self
    }

    /// Merge another set of decorators into this builder.
    ///
    /// Used by the derive macro together with [`DecoProbe`] for safe
    /// compile-time decorator collection from field types that may or may
    /// not implement [`HasSpytialDecorators`].
    pub fn extend_with(mut self, other: SpytialDecorators) -> Self {
        self.constraints.extend(other.constraints);
        self.directives.extend(other.directives);
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
                        selector: "value".to_string(),
                        directions: vec!["above".to_string()],
                        negated: false,
                    },
                }),
                Constraint::Align(AlignConstraint {
                    align: AlignParams {
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
