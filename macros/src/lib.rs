use proc_macro::TokenStream;
use quote::quote;
use syn::{
    parse_macro_input, Attribute, Data, DeriveInput, Fields, GenericArgument, PathArguments, Type,
};

/// Collect decorators from field types at compile time
/// This walks the type tree and generates calls to collect decorators from nested types
fn collect_field_type_decorators(
    data: &Data,
    self_type_name: &str,
) -> Vec<proc_macro2::TokenStream> {
    let mut field_decorators = Vec::new();
    let mut seen_types = std::collections::HashSet::new();

    // Add the self type to seen_types to prevent self-referential includes
    seen_types.insert(self_type_name.to_string());

    if let Data::Struct(data_struct) = data {
        match &data_struct.fields {
            Fields::Named(fields) => {
                for field in &fields.named {
                    field_decorators.extend(analyze_field_type(&field.ty, &mut seen_types));
                }
            }
            Fields::Unnamed(fields) => {
                for field in &fields.unnamed {
                    field_decorators.extend(analyze_field_type(&field.ty, &mut seen_types));
                }
            }
            Fields::Unit => {}
        }
    }

    field_decorators
}

/// Analyze a field type and generate decorator-collection calls for nested types.
///
/// Containers (`Vec`, `Option`, `Box`, `Rc`, `Arc`, `RefCell`, `Cell`,
/// `VecDeque`, `LinkedList`) are unwrapped to reach the inner type.
/// Primitives and standard collections are skipped (they can never carry
/// decorators).  Everything else gets a probe call via [`DecoProbe`] — if the
/// type implements `HasSpytialDecorators` the real decorators are returned;
/// otherwise the probe safely returns an empty set.
fn analyze_field_type(
    ty: &Type,
    seen_types: &mut std::collections::HashSet<String>,
) -> Vec<proc_macro2::TokenStream> {
    match ty {
        Type::Path(type_path) => {
            if let Some(segment) = type_path.path.segments.last() {
                let name = segment.ident.to_string();
                match name.as_str() {
                    // Containers: unwrap to reach the inner type
                    "Vec" | "Option" | "Box" | "Rc" | "Arc" | "RefCell" | "Cell" | "VecDeque"
                    | "LinkedList" => {
                        if let PathArguments::AngleBracketed(args) = &segment.arguments {
                            if let Some(GenericArgument::Type(inner)) = args.args.first() {
                                return analyze_inner_type(inner, seen_types);
                            }
                        }
                    }
                    // Primitives and std collections: can never have decorators
                    "i8" | "i16" | "i32" | "i64" | "i128" | "isize" | "u8" | "u16" | "u32"
                    | "u64" | "u128" | "usize" | "f32" | "f64" | "bool" | "char" | "String"
                    | "str" | "Result" | "HashMap" | "HashSet" | "BTreeMap" | "BTreeSet" => {}
                    // Everything else: safe to probe
                    _ => {
                        if !seen_types.contains(&name) {
                            seen_types.insert(name.clone());
                            return vec![generate_probe_call(&name)];
                        }
                    }
                }
            }
        }
        _ => {}
    }
    Vec::new()
}

/// Recursively unwrap container generics (`Option<Box<T>>` → `T`), then
/// probe the inner type.
fn analyze_inner_type(
    ty: &Type,
    seen_types: &mut std::collections::HashSet<String>,
) -> Vec<proc_macro2::TokenStream> {
    if let Type::Path(type_path) = ty {
        if let Some(segment) = type_path.path.segments.last() {
            let name = segment.ident.to_string();
            match name.as_str() {
                "Vec" | "Option" | "Box" | "Rc" | "Arc" | "RefCell" | "Cell" | "VecDeque"
                | "LinkedList" => {
                    if let PathArguments::AngleBracketed(args) = &segment.arguments {
                        if let Some(GenericArgument::Type(inner)) = args.args.first() {
                            return analyze_inner_type(inner, seen_types);
                        }
                    }
                }
                "i8" | "i16" | "i32" | "i64" | "i128" | "isize" | "u8" | "u16" | "u32" | "u64"
                | "u128" | "usize" | "f32" | "f64" | "bool" | "char" | "String" | "str"
                | "Result" | "HashMap" | "HashSet" | "BTreeMap" | "BTreeSet" => {}
                _ => {
                    if !seen_types.contains(&name) {
                        seen_types.insert(name.clone());
                        return vec![generate_probe_call(&name)];
                    }
                }
            }
        }
    }
    Vec::new()
}

/// Generate a probe call that safely collects decorators from `type_name`.
///
/// Uses the inherent-method-priority trick: if the type implements
/// `HasSpytialDecorators`, the inherent `DecoProbe::get` is chosen and
/// returns real decorators.  Otherwise the blanket `DefaultDecorators::get`
/// is chosen and returns an empty set.  No heuristic needed.
fn generate_probe_call(type_name: &str) -> proc_macro2::TokenStream {
    let type_ident = syn::Ident::new(type_name, proc_macro2::Span::call_site());
    quote! {
        .extend_with({
            use spytial::spytial_annotations::DefaultDecorators as _;
            spytial::spytial_annotations::DecoProbe::<#type_ident>(::std::marker::PhantomData).get()
        })
    }
}

/// Derive macro for implementing HasSpytialDecorators trait
///
/// This macro analyzes all spatial annotation attributes on a struct
/// and generates a single implementation of HasSpytialDecorators that includes
/// all the annotations.
///
/// # Supported Attributes
/// - `#[attribute(field = "field_name")]` - Adds attribute directive
/// - `#[flag(name = "flag_name")]` - Adds flag directive  
/// - `#[orientation(selector = "sel", directions = ["up", "down"], negated = true)]` - Adds orientation constraint (`negated` optional)
/// - `#[align(selector = "sel", direction = "horizontal", negated = true)]` - Adds align constraint (`negated` optional)
/// - `#[cyclic(selector = "sel", direction = "up", negated = true)]` - Adds cyclic constraint (`negated` optional)
/// - `#[group(selector = "sel", name = "group_name", negated = true)]` - Adds selector-based group constraint (`negated` optional)
/// - `#[group(field = "field", group_on = 1, add_to_group = 2, negated = true)]` - Adds field-based group constraint (`negated` optional)
/// - `#[atom_color(selector = "sel", value = "red")]` - Adds atom color directive
/// - `#[size(selector = "sel", height = 20, width = 30)]` - Adds size directive
/// - `#[icon(selector = "sel", path = "icon.png", show_labels = true)]` - Adds icon directive
/// - `#[edge_style(field = "field", value = "blue", style = "dashed", weight = 2.0, show_label = true, hidden = false, filter = "...", selector = "...")]` - Adds edge style directive (replaces edge_color)
/// - `#[projection(sig = "signature")]` - Adds projection directive
/// - `#[hide_field(field = "field")]` - Adds hide field directive
/// - `#[hide_atom(selector = "sel")]` - Adds hide atom directive
/// - `#[inferred_edge(name = "edge", selector = "sel")]` - Adds inferred edge directive
/// - `#[tag(to_tag = "sel", name = "attr", value = "n-ary selector")]` - Adds tag directive
///
/// # Example
/// ```rust
/// use serde::Serialize;
/// use spytial::SpytialDecorators;
///
/// #[derive(Serialize, SpytialDecorators)]
/// #[attribute(field = "name")]
/// #[flag(name = "important")]
/// struct Person {
///     name: String,
///     age: u32,
/// }
/// ```
#[proc_macro_derive(
    SpytialDecorators,
    attributes(
        attribute,
        flag,
        orientation,
        align,
        cyclic,
        group,
        atom_color,
        size,
        icon,
        edge_style,
        projection,
        hide_field,
        hide_atom,
        inferred_edge,
        tag
    )
)]
pub fn derive_spytial_decorators(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let name = &input.ident;
    let generics = &input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    // Parse spatial annotation attributes for this type
    let mut decorator_calls = Vec::new();

    for attr in &input.attrs {
        let parsed = match parse_spatial_attribute(attr) {
            Ok(parsed) => parsed,
            Err(err) => return err.to_compile_error().into(),
        };
        match parsed {
            Some(SpatialAttribute::Attribute { field }) => {
                decorator_calls.push(quote! {
                    .attribute(#field, None)
                });
            }
            Some(SpatialAttribute::Flag { name }) => {
                decorator_calls.push(quote! {
                    .flag(#name)
                });
            }
            Some(SpatialAttribute::Orientation {
                selector,
                directions,
                negated,
            }) => {
                decorator_calls.push(quote! {
                    .orientation(#selector, vec![#(#directions),*], #negated)
                });
            }
            Some(SpatialAttribute::Align {
                selector,
                direction,
                negated,
            }) => {
                decorator_calls.push(quote! {
                    .align(#selector, #direction, #negated)
                });
            }
            Some(SpatialAttribute::Cyclic {
                selector,
                direction,
                negated,
            }) => {
                decorator_calls.push(quote! {
                    .cyclic(#selector, #direction, #negated)
                });
            }
            Some(SpatialAttribute::GroupSelector {
                selector,
                name,
                negated,
            }) => {
                decorator_calls.push(quote! {
                    .group_selector_based(#selector, #name, #negated)
                });
            }
            Some(SpatialAttribute::GroupField {
                field,
                group_on,
                add_to_group,
                negated,
            }) => {
                decorator_calls.push(quote! {
                    .group_field_based(#field, #group_on, #add_to_group, None, #negated)
                });
            }
            Some(SpatialAttribute::AtomColor { selector, value }) => {
                decorator_calls.push(quote! {
                    .atom_color(#selector, #value)
                });
            }
            Some(SpatialAttribute::Size {
                selector,
                height,
                width,
            }) => {
                decorator_calls.push(quote! {
                    .size(#selector, #height, #width)
                });
            }
            Some(SpatialAttribute::Icon {
                selector,
                path,
                show_labels,
            }) => {
                decorator_calls.push(quote! {
                    .icon(#selector, #path, #show_labels)
                });
            }
            Some(SpatialAttribute::EdgeStyle {
                field,
                value,
                selector,
                filter,
                style,
                weight,
                show_label,
                hidden,
            }) => {
                let opt_str = |v: Option<String>| match v {
                    Some(s) => quote! { Some(#s) },
                    None => quote! { None },
                };
                let opt_f64 = |v: Option<f64>| match v {
                    Some(n) => quote! { Some(#n) },
                    None => quote! { None },
                };
                let opt_bool = |v: Option<bool>| match v {
                    Some(b) => quote! { Some(#b) },
                    None => quote! { None },
                };
                let selector_arg = opt_str(selector);
                let filter_arg = opt_str(filter);
                let style_arg = opt_str(style);
                let weight_arg = opt_f64(weight);
                let show_label_arg = opt_bool(show_label);
                let hidden_arg = opt_bool(hidden);
                decorator_calls.push(quote! {
                    .edge_style(
                        #field,
                        #value,
                        #selector_arg,
                        #filter_arg,
                        #style_arg,
                        #weight_arg,
                        #show_label_arg,
                        #hidden_arg,
                    )
                });
            }
            Some(SpatialAttribute::Projection { sig }) => {
                decorator_calls.push(quote! {
                    .projection(#sig)
                });
            }
            Some(SpatialAttribute::HideField { field, selector }) => {
                let selector_arg = match selector {
                    Some(s) => quote! { Some(#s) },
                    None => quote! { None },
                };
                decorator_calls.push(quote! {
                    .hide_field(#field, #selector_arg)
                });
            }
            Some(SpatialAttribute::HideAtom { selector }) => {
                decorator_calls.push(quote! {
                    .hide_atom(#selector)
                });
            }
            Some(SpatialAttribute::InferredEdge { name, selector }) => {
                decorator_calls.push(quote! {
                    .inferred_edge(#name, #selector)
                });
            }
            Some(SpatialAttribute::Tag {
                to_tag,
                name,
                value,
            }) => {
                decorator_calls.push(quote! {
                    .tag(#to_tag, #name, #value)
                });
            }
            None => {}
        }
    }

    // Second, analyze field types and collect their decorators at compile time
    // Only do this for structs - enums don't have fields to analyze
    let field_type_decorators = match &input.data {
        Data::Struct(_) => collect_field_type_decorators(&input.data, &name.to_string()),
        Data::Enum(_) | Data::Union(_) => Vec::new(), // Enums and unions just return empty decorators
    };

    // Combine own decorators with field type decorators
    decorator_calls.extend(field_type_decorators);

    // Generate the HasSpytialDecorators implementation
    let expanded = quote! {
        impl #impl_generics spytial::spytial_annotations::HasSpytialDecorators for #name #ty_generics #where_clause {
            fn decorators() -> spytial::spytial_annotations::SpytialDecorators {
                // Register this type automatically when decorators() is called
                static REGISTRATION: ::std::sync::Once = ::std::sync::Once::new();
                REGISTRATION.call_once(|| {
                    let decorators = spytial::spytial_annotations::SpytialDecoratorsBuilder::new()
                        #(#decorator_calls)*
                        .build();
                    spytial::spytial_annotations::register_type_decorators(
                        stringify!(#name),
                        decorators.clone()
                    );
                });

                spytial::spytial_annotations::SpytialDecoratorsBuilder::new()
                    #(#decorator_calls)*
                    .build()
            }
        }
    };

    TokenStream::from(expanded)
}

#[derive(Debug)]
enum SpatialAttribute {
    Attribute {
        field: String,
    },
    Flag {
        name: String,
    },
    Orientation {
        selector: String,
        directions: Vec<String>,
        negated: bool,
    },
    Align {
        selector: String,
        direction: String,
        negated: bool,
    },
    Cyclic {
        selector: String,
        direction: String,
        negated: bool,
    },
    GroupSelector {
        selector: String,
        name: String,
        negated: bool,
    },
    GroupField {
        field: String,
        group_on: u32,
        add_to_group: u32,
        negated: bool,
    },
    AtomColor {
        selector: String,
        value: String,
    },
    Size {
        selector: String,
        height: u32,
        width: u32,
    },
    Icon {
        selector: String,
        path: String,
        show_labels: bool,
    },
    EdgeStyle {
        field: String,
        value: String,
        selector: Option<String>,
        filter: Option<String>,
        style: Option<String>,
        weight: Option<f64>,
        show_label: Option<bool>,
        hidden: Option<bool>,
    },
    Projection {
        sig: String,
    },
    HideField {
        field: String,
        selector: Option<String>,
    },
    HideAtom {
        selector: String,
    },
    InferredEdge {
        name: String,
        selector: String,
    },
    Tag {
        to_tag: String,
        name: String,
        value: String,
    },
}

fn parse_spatial_attribute(attr: &Attribute) -> Result<Option<SpatialAttribute>, syn::Error> {
    let path = &attr.path();

    if path.is_ident("attribute") {
        parse_attribute_args(attr)
    } else if path.is_ident("flag") {
        parse_flag_args(attr)
    } else if path.is_ident("orientation") {
        parse_orientation_args(attr)
    } else if path.is_ident("align") {
        parse_align_args(attr)
    } else if path.is_ident("cyclic") {
        parse_cyclic_args(attr)
    } else if path.is_ident("group") {
        parse_group_args(attr)
    } else if path.is_ident("atom_color") {
        parse_atom_color_args(attr)
    } else if path.is_ident("size") {
        parse_size_args(attr)
    } else if path.is_ident("icon") {
        parse_icon_args(attr)
    } else if path.is_ident("edge_style") {
        parse_edge_style_args(attr)
    } else if path.is_ident("projection") {
        parse_projection_args(attr)
    } else if path.is_ident("hide_field") {
        parse_hide_field_args(attr)
    } else if path.is_ident("hide_atom") {
        parse_hide_atom_args(attr)
    } else if path.is_ident("inferred_edge") {
        parse_inferred_edge_args(attr)
    } else if path.is_ident("tag") {
        parse_tag_args(attr)
    } else {
        Ok(None)
    }
}

/// Walk the meta items of `attr` and emit a `syn::Error` (pointing at the
/// offending key's span) for any key that isn't in `known`.  This catches typos
/// like `#[orientation(typo = "...")]` at compile time instead of silently
/// falling back to defaults.
///
/// The value side of each pair is consumed but not interpreted; the existing
/// string-based extractors handle the actual value parsing.
fn validate_known_keys(
    attr: &Attribute,
    attr_name: &str,
    known: &[&str],
) -> Result<(), syn::Error> {
    // Attributes like `#[flag]` with no list body have no keys to check.
    if attr.meta.require_list().is_err() {
        return Ok(());
    }

    attr.parse_nested_meta(|meta| {
        let ident = match meta.path.get_ident() {
            Some(ident) => ident,
            None => return Ok(()),
        };
        let key = ident.to_string();
        if !known.iter().any(|k| *k == key) {
            return Err(syn::Error::new(
                ident.span(),
                format!(
                    "unknown parameter `{}` for #[{}(...)]; expected one of: {}",
                    key,
                    attr_name,
                    known.join(", "),
                ),
            ));
        }
        // Consume the value (if any) so parse_nested_meta advances correctly.
        // We use `syn::Expr` rather than `TokenStream` because the latter
        // greedily eats the rest of the attribute (including subsequent
        // key/value pairs).  Tolerate parse failures here — we only care about
        // validating keys; any malformed value will surface elsewhere.
        if let Ok(value) = meta.value() {
            let _ = value.parse::<syn::Expr>();
        }
        Ok(())
    })
}

fn parse_attribute_args(attr: &Attribute) -> Result<Option<SpatialAttribute>, syn::Error> {
    validate_known_keys(attr, "attribute", &["field"])?;
    // Simple parsing - look for field = "value"
    if let Ok(meta) = attr.meta.require_list() {
        let tokens = &meta.tokens;
        let token_str = tokens.to_string();

        if let Some(field) = extract_string_from_tokens(&token_str, "field") {
            return Ok(Some(SpatialAttribute::Attribute { field }));
        }
    }

    Ok(Some(SpatialAttribute::Attribute {
        field: "name".to_string(),
    }))
}

fn parse_flag_args(attr: &Attribute) -> Result<Option<SpatialAttribute>, syn::Error> {
    validate_known_keys(attr, "flag", &["name"])?;
    if let Ok(meta) = attr.meta.require_list() {
        let tokens = &meta.tokens;
        let token_str = tokens.to_string();

        if let Some(name) = extract_string_from_tokens(&token_str, "name") {
            return Ok(Some(SpatialAttribute::Flag { name }));
        }
    }

    Ok(Some(SpatialAttribute::Flag {
        name: "important".to_string(),
    }))
}

fn parse_orientation_args(attr: &Attribute) -> Result<Option<SpatialAttribute>, syn::Error> {
    validate_known_keys(attr, "orientation", &["selector", "directions", "negated"])?;
    if let Ok(meta) = attr.meta.require_list() {
        let tokens = &meta.tokens;
        let token_str = normalize_whitespace(&tokens.to_string());

        let selector =
            extract_string_from_tokens(&token_str, "selector").unwrap_or_else(|| "".to_string());
        let directions = extract_array_from_tokens(&token_str, "directions")
            .unwrap_or_else(|| vec!["up".to_string(), "down".to_string()]);
        let negated = extract_bool_from_tokens(&token_str, "negated").unwrap_or(false);

        return Ok(Some(SpatialAttribute::Orientation {
            selector,
            directions,
            negated,
        }));
    }

    Ok(None)
}

fn parse_group_args(attr: &Attribute) -> Result<Option<SpatialAttribute>, syn::Error> {
    // `group` accepts two shapes (selector-based or field-based); accept the
    // union of valid keys here and let the body pick the right variant.
    validate_known_keys(
        attr,
        "group",
        &[
            "selector",
            "name",
            "field",
            "group_on",
            "add_to_group",
            "negated",
        ],
    )?;
    if let Ok(meta) = attr.meta.require_list() {
        let tokens = &meta.tokens;
        let token_str = normalize_whitespace(&tokens.to_string());
        let negated = extract_bool_from_tokens(&token_str, "negated").unwrap_or(false);

        if token_str.contains("field =") {
            // Field-based grouping
            let field =
                extract_string_from_tokens(&token_str, "field").unwrap_or_else(|| "id".to_string());
            let group_on = extract_number_from_tokens(&token_str, "group_on").unwrap_or(1);
            let add_to_group = extract_number_from_tokens(&token_str, "add_to_group").unwrap_or(2);

            Ok(Some(SpatialAttribute::GroupField {
                field,
                group_on,
                add_to_group,
                negated,
            }))
        } else {
            // Selector-based grouping
            let selector = extract_string_from_tokens(&token_str, "selector")
                .unwrap_or_else(|| "".to_string());
            let name = extract_string_from_tokens(&token_str, "name")
                .unwrap_or_else(|| "default".to_string());

            Ok(Some(SpatialAttribute::GroupSelector {
                selector,
                name,
                negated,
            }))
        }
    } else {
        Ok(None)
    }
}

fn parse_align_args(attr: &Attribute) -> Result<Option<SpatialAttribute>, syn::Error> {
    validate_known_keys(attr, "align", &["selector", "direction", "negated"])?;
    if let Ok(meta) = attr.meta.require_list() {
        let tokens = &meta.tokens;
        let token_str = normalize_whitespace(&tokens.to_string());

        let selector =
            extract_string_from_tokens(&token_str, "selector").unwrap_or_else(|| "".to_string());
        let direction = extract_string_from_tokens(&token_str, "direction")
            .unwrap_or_else(|| "horizontal".to_string());
        let negated = extract_bool_from_tokens(&token_str, "negated").unwrap_or(false);

        Ok(Some(SpatialAttribute::Align {
            selector,
            direction,
            negated,
        }))
    } else {
        Ok(None)
    }
}

fn parse_cyclic_args(attr: &Attribute) -> Result<Option<SpatialAttribute>, syn::Error> {
    validate_known_keys(attr, "cyclic", &["selector", "direction", "negated"])?;
    if let Ok(meta) = attr.meta.require_list() {
        let tokens = &meta.tokens;
        let token_str = normalize_whitespace(&tokens.to_string());

        let selector =
            extract_string_from_tokens(&token_str, "selector").unwrap_or_else(|| "".to_string());
        let direction =
            extract_string_from_tokens(&token_str, "direction").unwrap_or_else(|| "up".to_string());
        let negated = extract_bool_from_tokens(&token_str, "negated").unwrap_or(false);

        Ok(Some(SpatialAttribute::Cyclic {
            selector,
            direction,
            negated,
        }))
    } else {
        Ok(None)
    }
}

fn parse_atom_color_args(attr: &Attribute) -> Result<Option<SpatialAttribute>, syn::Error> {
    validate_known_keys(attr, "atom_color", &["selector", "value"])?;
    if let Ok(meta) = attr.meta.require_list() {
        let tokens = &meta.tokens;
        let token_str = tokens.to_string();

        let selector =
            extract_string_from_tokens(&token_str, "selector").unwrap_or_else(|| "".to_string());
        let value =
            extract_string_from_tokens(&token_str, "value").unwrap_or_else(|| "blue".to_string());

        Ok(Some(SpatialAttribute::AtomColor { selector, value }))
    } else {
        Ok(None)
    }
}

fn parse_size_args(attr: &Attribute) -> Result<Option<SpatialAttribute>, syn::Error> {
    validate_known_keys(attr, "size", &["selector", "height", "width"])?;
    if let Ok(meta) = attr.meta.require_list() {
        let tokens = &meta.tokens;
        let token_str = tokens.to_string();

        let selector =
            extract_string_from_tokens(&token_str, "selector").unwrap_or_else(|| "".to_string());
        let height = extract_number_from_tokens(&token_str, "height").unwrap_or(20);
        let width = extract_number_from_tokens(&token_str, "width").unwrap_or(30);

        Ok(Some(SpatialAttribute::Size {
            selector,
            height,
            width,
        }))
    } else {
        Ok(None)
    }
}

fn parse_icon_args(attr: &Attribute) -> Result<Option<SpatialAttribute>, syn::Error> {
    validate_known_keys(attr, "icon", &["selector", "path", "show_labels"])?;
    if let Ok(meta) = attr.meta.require_list() {
        let tokens = &meta.tokens;
        let token_str = tokens.to_string();

        let selector =
            extract_string_from_tokens(&token_str, "selector").unwrap_or_else(|| "".to_string());
        let path = extract_string_from_tokens(&token_str, "path")
            .unwrap_or_else(|| "icon.png".to_string());
        let show_labels = extract_bool_from_tokens(&token_str, "show_labels").unwrap_or(true);

        Ok(Some(SpatialAttribute::Icon {
            selector,
            path,
            show_labels,
        }))
    } else {
        Ok(None)
    }
}

fn parse_edge_style_args(attr: &Attribute) -> Result<Option<SpatialAttribute>, syn::Error> {
    validate_known_keys(
        attr,
        "edge_style",
        &[
            "field",
            "value",
            "selector",
            "filter",
            "style",
            "weight",
            "show_label",
            "hidden",
        ],
    )?;
    if let Ok(meta) = attr.meta.require_list() {
        let tokens = &meta.tokens;
        let token_str = normalize_whitespace(&tokens.to_string());

        let field = extract_string_from_tokens(&token_str, "field")
            .unwrap_or_else(|| "relation".to_string());
        let value =
            extract_string_from_tokens(&token_str, "value").unwrap_or_else(|| "blue".to_string());
        let selector = extract_string_from_tokens(&token_str, "selector");
        let filter = extract_string_from_tokens(&token_str, "filter");
        let style = extract_string_from_tokens(&token_str, "style");
        let weight = extract_float_from_tokens(&token_str, "weight");
        let show_label = extract_bool_from_tokens(&token_str, "show_label");
        let hidden = extract_bool_from_tokens(&token_str, "hidden");

        Ok(Some(SpatialAttribute::EdgeStyle {
            field,
            value,
            selector,
            filter,
            style,
            weight,
            show_label,
            hidden,
        }))
    } else {
        Ok(None)
    }
}

fn parse_projection_args(attr: &Attribute) -> Result<Option<SpatialAttribute>, syn::Error> {
    validate_known_keys(attr, "projection", &["sig"])?;
    if let Ok(meta) = attr.meta.require_list() {
        let tokens = &meta.tokens;
        let token_str = tokens.to_string();

        let sig =
            extract_string_from_tokens(&token_str, "sig").unwrap_or_else(|| "default".to_string());

        Ok(Some(SpatialAttribute::Projection { sig }))
    } else {
        Ok(None)
    }
}

fn parse_hide_field_args(attr: &Attribute) -> Result<Option<SpatialAttribute>, syn::Error> {
    validate_known_keys(attr, "hide_field", &["field", "selector"])?;
    if let Ok(meta) = attr.meta.require_list() {
        let tokens = &meta.tokens;
        let token_str = tokens.to_string();

        let field =
            extract_string_from_tokens(&token_str, "field").unwrap_or_else(|| "field".to_string());
        let selector = extract_string_from_tokens(&token_str, "selector");

        Ok(Some(SpatialAttribute::HideField { field, selector }))
    } else {
        Ok(None)
    }
}

fn parse_hide_atom_args(attr: &Attribute) -> Result<Option<SpatialAttribute>, syn::Error> {
    validate_known_keys(attr, "hide_atom", &["selector"])?;
    if let Ok(meta) = attr.meta.require_list() {
        let tokens = &meta.tokens;
        let token_str = tokens.to_string();

        let selector =
            extract_string_from_tokens(&token_str, "selector").unwrap_or_else(|| "".to_string());

        Ok(Some(SpatialAttribute::HideAtom { selector }))
    } else {
        Ok(None)
    }
}

fn parse_inferred_edge_args(attr: &Attribute) -> Result<Option<SpatialAttribute>, syn::Error> {
    validate_known_keys(attr, "inferred_edge", &["name", "selector"])?;
    if let Ok(meta) = attr.meta.require_list() {
        let tokens = &meta.tokens;
        let token_str = tokens.to_string();

        let name =
            extract_string_from_tokens(&token_str, "name").unwrap_or_else(|| "edge".to_string());
        let selector =
            extract_string_from_tokens(&token_str, "selector").unwrap_or_else(|| "".to_string());

        Ok(Some(SpatialAttribute::InferredEdge { name, selector }))
    } else {
        Ok(None)
    }
}

fn parse_tag_args(attr: &Attribute) -> Result<Option<SpatialAttribute>, syn::Error> {
    validate_known_keys(attr, "tag", &["to_tag", "name", "value"])?;
    if let Ok(meta) = attr.meta.require_list() {
        let tokens = &meta.tokens;
        let token_str = tokens.to_string();

        let to_tag = extract_string_from_tokens(&token_str, "to_tag").unwrap_or_default();
        let name = extract_string_from_tokens(&token_str, "name").unwrap_or_default();
        let value = extract_string_from_tokens(&token_str, "value").unwrap_or_default();

        Ok(Some(SpatialAttribute::Tag {
            to_tag,
            name,
            value,
        }))
    } else {
        Ok(None)
    }
}

/// Replace newlines/tabs with spaces so the `extract_*_from_tokens` helpers
/// (which match `key = ` / `key = "`) work even when proc-macro2 wraps long
/// attribute lists across multiple lines.
fn normalize_whitespace(tokens: &str) -> String {
    tokens.replace(['\n', '\r', '\t'], " ")
}

fn extract_string_from_tokens(tokens: &str, key: &str) -> Option<String> {
    // Try both with and without spaces around =
    let patterns = [
        format!("{} = \"", key),
        format!("{}=\"", key),
        format!("{} =\"", key),
        format!("{}= \"", key),
    ];

    for pattern in &patterns {
        if let Some(start) = tokens.find(pattern) {
            let start = start + pattern.len();
            if let Some(end) = tokens[start..].find('"') {
                return Some(tokens[start..start + end].to_string());
            }
        }
    }
    None
}

fn extract_number_from_tokens(tokens: &str, key: &str) -> Option<u32> {
    let pattern = format!("{} = ", key);
    if let Some(start) = tokens.find(&pattern) {
        let start = start + pattern.len();
        let rest = &tokens[start..];
        let end = rest.find([',', ' ', ')']).unwrap_or(rest.len());
        if let Ok(value) = rest[..end].trim().parse::<u32>() {
            return Some(value);
        }
    }
    None
}

fn extract_bool_from_tokens(tokens: &str, key: &str) -> Option<bool> {
    let pattern = format!("{} = ", key);
    if let Some(start) = tokens.find(&pattern) {
        let start = start + pattern.len();
        let rest = &tokens[start..];
        let end = rest.find([',', ' ', ')']).unwrap_or(rest.len());
        if let Ok(value) = rest[..end].trim().parse::<bool>() {
            return Some(value);
        }
    }
    None
}

fn extract_float_from_tokens(tokens: &str, key: &str) -> Option<f64> {
    let pattern = format!("{} = ", key);
    if let Some(start) = tokens.find(&pattern) {
        let start = start + pattern.len();
        let rest = &tokens[start..];
        let end = rest.find([',', ' ', ')']).unwrap_or(rest.len());
        if let Ok(value) = rest[..end].trim().parse::<f64>() {
            return Some(value);
        }
    }
    None
}

fn extract_array_from_tokens(tokens: &str, key: &str) -> Option<Vec<String>> {
    // Try different patterns since the tokenizer might have different spacing
    let patterns = [
        format!("{}=[", key),
        format!("{} = [", key),
        format!("{}= [", key),
        format!("{} =[", key),
    ];

    for pattern in &patterns {
        if let Some(start) = tokens.find(pattern) {
            let start = start + pattern.len();
            let rest = &tokens[start..];
            if let Some(end) = rest.find(']') {
                let array_content = &rest[..end];
                let items: Vec<String> = array_content
                    .split(',')
                    .map(|s| s.trim().trim_matches('"').to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                return Some(items);
            }
        }
    }
    None
}
