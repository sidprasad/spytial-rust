use proc_macro::TokenStream;
use quote::{quote, quote_spanned, ToTokens};
use syn::{
    parse_macro_input, punctuated::Punctuated, spanned::Spanned, Attribute, Data, DeriveInput,
    Expr, Fields, GenericArgument, Lit, Meta, PathArguments, Token, Type,
};

mod spec_tables;
use spec_tables::{DeprecationSpec, FieldRule};

/// Return the name Serde passes to `Serializer::serialize_*` for this type.
///
/// Serde defaults to the Rust identifier and permits an explicit type-level
/// `rename`, including a serialization-only rename. Reading that attribute in
/// the derive keeps registry lookup aligned with the structured-data boundary
/// without asking users to repeat a type name manually.
fn serialized_type_name(attrs: &[Attribute], default: &str) -> syn::Result<String> {
    let mut serialized_name = default.to_string();

    for attr in attrs.iter().filter(|attr| attr.path().is_ident("serde")) {
        let Meta::List(list) = &attr.meta else {
            continue;
        };
        let metas = list.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)?;
        for meta in metas {
            match meta {
                Meta::NameValue(name_value) if name_value.path.is_ident("rename") => {
                    if let Expr::Lit(expr) = name_value.value {
                        if let Lit::Str(name) = expr.lit {
                            serialized_name = name.value();
                        }
                    }
                }
                Meta::List(rename) if rename.path.is_ident("rename") => {
                    let directions =
                        rename.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)?;
                    for direction in directions {
                        if let Meta::NameValue(name_value) = direction {
                            if name_value.path.is_ident("serialize") {
                                if let Expr::Lit(expr) = name_value.value {
                                    if let Lit::Str(name) = expr.lit {
                                        serialized_name = name.value();
                                    }
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    Ok(serialized_name)
}

/// Emit a decorator-probe call for each distinct user type reachable through
/// this type's fields (looking through containers like `Vec`/`Option`/`Box`).
fn collect_field_type_decorators(
    data: &Data,
    self_type_name: &str,
) -> Vec<(String, proc_macro2::TokenStream)> {
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
/// `VecDeque`, `LinkedList`) are unwrapped recursively to reach the inner type
/// (`Option<Box<T>>` → `T`). Primitives and standard collections are skipped
/// (they can never carry decorators). Everything else gets a probe call via
/// [`DecoProbe`] — if the type implements `HasSpytialDecorators` the real
/// decorators are returned; otherwise the probe safely returns an empty set.
fn analyze_field_type(
    ty: &Type,
    seen_types: &mut std::collections::HashSet<String>,
) -> Vec<(String, proc_macro2::TokenStream)> {
    let Type::Path(type_path) = ty else {
        return Vec::new();
    };
    let Some(segment) = type_path.path.segments.last() else {
        return Vec::new();
    };
    let name = segment.ident.to_string();
    match name.as_str() {
        // Containers: unwrap to reach the inner type
        "Vec" | "Option" | "Box" | "Rc" | "Arc" | "RefCell" | "Cell" | "VecDeque"
        | "LinkedList" => {
            if let PathArguments::AngleBracketed(args) = &segment.arguments {
                if let Some(GenericArgument::Type(inner)) = args.args.first() {
                    return analyze_field_type(inner, seen_types);
                }
            }
            Vec::new()
        }
        // Primitives and std collections: can never have decorators
        "i8" | "i16" | "i32" | "i64" | "i128" | "isize" | "u8" | "u16" | "u32" | "u64" | "u128"
        | "usize" | "f32" | "f64" | "bool" | "char" | "String" | "str" | "Result" | "HashMap"
        | "HashSet" | "BTreeMap" | "BTreeSet" => Vec::new(),
        // Everything else: safe to probe
        _ => {
            if seen_types.insert(name.clone()) {
                vec![(name.clone(), generate_probe_call(&name))]
            } else {
                Vec::new()
            }
        }
    }
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

/// Derive `HasSpytialDecorators`, turning a type's spatial-annotation attributes
/// into a single `decorators()` impl that also pulls in the decorators of nested
/// field types.
///
/// # Supported Attributes
///
/// Which keys each attribute accepts, and which values are legal, come from
/// spytial-core's own language manifest — see `spec_tables.rs` and
/// `spec-codegen/`. A key or value this crate rejects is one spytial-core would
/// reject or silently ignore, so a typo fails here rather than rendering a
/// diagram quietly missing what you asked for. The reference below is
/// generated from the same manifest (`macros/src/attributes.md`).
///
/// Every rule the derive emits is stamped with a `source` block: the attribute
/// as you wrote it and its `file:line`. spytial-core's conflict reports and
/// warnings cite that text, so a report names your `#[orientation(...)]` at
/// its line rather than a reconstructed YAML fragment.
///
/// Styling uses the spytial-core 3.x nested blocks, written as groups that
/// mirror the YAML 1:1: `line_style(color = ..., pattern = ..., weight = ...,
/// highlight = ...)`, `text_style(size = ..., color = ...)`,
/// `border_style(color = ..., width = ...)`, `fill_style(color = ...)`,
/// `icon_style(path = ..., placement = ..., opacity = ...)`.
///
#[doc = include_str!("attributes.md")]
///
/// # Example
/// ```rust
/// use serde::Serialize;
/// use spytial::SpytialDecorators;
///
/// #[derive(Serialize, SpytialDecorators)]
/// #[attribute(field = "name")]
/// #[flag(name = "hideDisconnected")]
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
        atom_style,
        size,
        icon,
        edge_style,
        hide_field,
        hide_atom,
        inferred_edge,
        tag
    )
)]
pub fn derive_spytial_decorators(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let name = &input.ident;
    let serialized_name = match serialized_type_name(&input.attrs, &name.to_string()) {
        Ok(name) => name,
        Err(err) => return err.to_compile_error().into(),
    };
    let generics = &input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    // Parse spatial annotation attributes for this type
    let mut decorator_calls = Vec::new();
    let mut deprecation_shims = Vec::new();

    // Suppressing lint attributes the user wrote on the type, copied onto every
    // deprecation shim below.
    //
    // A shim is a sibling item of the struct, not part of it, so a plain
    // `#[allow(deprecated)]` on the struct does not reach it — without this the
    // only way to quiet one legacy attribute would be `#![allow(deprecated)]`
    // over the whole module, which also hides every unrelated deprecation in
    // it. Escalation needs no help: a module- or crate-level `deny` already
    // covers the shim, because the module *is* its lint parent.
    let lint_attrs: Vec<&Attribute> = input
        .attrs
        .iter()
        .filter(|a| a.path().is_ident("allow") || a.path().is_ident("expect"))
        .collect();

    for attr in &input.attrs {
        let parsed = match parse_spatial_attribute(attr) {
            Ok(parsed) => parsed,
            Err(err) => return err.to_compile_error().into(),
        };
        // Only after a clean parse: a malformed attribute should report the
        // error it has, not a deprecation notice on top of it.
        if parsed.is_some() {
            if let Some(name) = attr.path().get_ident().map(|i| i.to_string()) {
                if let Some(shim) = deprecation_shim(attr, &name) {
                    deprecation_shims.push(quote! { #(#lint_attrs)* #shim });
                }
            }
        }
        let is_rule = parsed.is_some();
        match parsed {
            Some(SpatialAttribute::Attribute {
                field,
                selector,
                filter,
                text_style,
            }) => {
                let selector_arg = quote_opt_str_ref(&selector);
                let filter_arg = quote_opt_str_ref(&filter);
                let ts = quote_text_style(&text_style);
                decorator_calls.push(quote! {
                    .attribute_styled(#field, #selector_arg, #filter_arg, #ts)
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
                add_edge,
                show_label,
                text_style,
                negated,
            }) => {
                let ae = quote_add_edge(&add_edge);
                let sl = match show_label {
                    Some(b) => quote! { Some(#b) },
                    None => quote! { None },
                };
                let ts = quote_text_style(&text_style);
                decorator_calls.push(quote! {
                    .group_selector_based_styled_with_label(#selector, #name, #ae, #sl, #ts, #negated)
                });
            }
            Some(SpatialAttribute::AtomColor { selector, value }) => {
                decorator_calls.push(quote! {
                    .atom_color(#selector, #value)
                });
            }
            Some(SpatialAttribute::AtomStyle {
                selector,
                fill_style,
                border_style,
                icon_style,
                text_style,
                show_label,
            }) => {
                let selector_arg = quote_opt_str_ref(&selector);
                let fs = quote_fill_style(&fill_style);
                let bs = quote_border_style(&border_style);
                let is = quote_icon_style(&icon_style);
                let ts = quote_text_style(&text_style);
                let sl = match show_label {
                    Some(b) => quote! { Some(#b) },
                    None => quote! { None },
                };
                decorator_calls.push(quote! {
                    .atom_style(#selector_arg, #fs, #bs, #is, #ts, #sl)
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
            Some(SpatialAttribute::EdgeStyleLegacy {
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
                    .edge_color(
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
            Some(SpatialAttribute::EdgeStyle {
                field,
                selector,
                filter,
                line_style,
                text_style,
                show_label,
                hidden,
            }) => {
                let opt_str = |v: Option<String>| match v {
                    Some(s) => quote! { Some(#s) },
                    None => quote! { None },
                };
                let opt_bool = |v: Option<bool>| match v {
                    Some(b) => quote! { Some(#b) },
                    None => quote! { None },
                };
                let selector_arg = opt_str(selector);
                let filter_arg = opt_str(filter);
                let ls = quote_line_style(&line_style);
                let ts = quote_text_style(&text_style);
                let show_label_arg = opt_bool(show_label);
                let hidden_arg = opt_bool(hidden);
                decorator_calls.push(quote! {
                    .edge_style(
                        #field,
                        #selector_arg,
                        #filter_arg,
                        #ls,
                        #ts,
                        #show_label_arg,
                        #hidden_arg,
                    )
                });
            }
            Some(SpatialAttribute::HideField {
                field,
                selector,
                filter,
            }) => {
                let selector_arg = quote_opt_str_ref(&selector);
                let filter_arg = quote_opt_str_ref(&filter);
                decorator_calls.push(quote! {
                    .hide_field(#field, #selector_arg, #filter_arg)
                });
            }
            Some(SpatialAttribute::HideAtom { selector }) => {
                decorator_calls.push(quote! {
                    .hide_atom(#selector)
                });
            }
            Some(SpatialAttribute::InferredEdge {
                name,
                selector,
                draw,
                line_style,
                text_style,
            }) => {
                let dr = quote_draw(&draw);
                let ls = quote_line_style(&line_style);
                let ts = quote_text_style(&text_style);
                decorator_calls.push(quote! {
                    .inferred_edge_drawn(#name, #selector, #dr, #ls, #ts)
                });
            }
            Some(SpatialAttribute::Tag {
                to_tag,
                name,
                value,
                text_style,
            }) => {
                let ts = quote_text_style(&text_style);
                decorator_calls.push(quote! {
                    .tag_styled(#to_tag, #name, #value, #ts)
                });
            }
            None => {}
        }
        if is_rule {
            // spytial-core 5.4's `source` block: the rule as written and where.
            // `file!()`/`line!()` resolve at the span they are given, so
            // spanning them to the attribute reports the attribute's own line
            // rather than the derive's.
            let text = render_attribute(attr);
            let span = attr.span();
            let location = quote_spanned! {span=>
                ::std::concat!(::std::file!(), ":", ::std::line!())
            };
            decorator_calls.push(quote! {
                .source(#text, ::std::option::Option::Some(#location))
            });
        }
    }

    let own_decorator_calls = decorator_calls.clone();

    // Analyze field types and collect their decorators at compile time.
    // Only structs have fields to walk; enums and unions contribute nothing.
    let field_type_decorators = match &input.data {
        Data::Struct(_) => collect_field_type_decorators(&input.data, &name.to_string()),
        Data::Enum(_) | Data::Union(_) => Vec::new(),
    };

    // Combine own decorators with field type decorators.
    decorator_calls.extend(field_type_decorators.into_iter().map(|(_, call)| call));

    // A concrete type can report the exact identity used by
    // `std::any::type_name`. Generic registrations are emitted only once, not
    // once per monomorphization, so they use their qualified constructor name
    // and the runtime treats it as a generic prefix.
    let has_generic_parameters = !generics.params.is_empty();
    // A link-time registration is emitted outside the generated generic impl,
    // so generic parameters are unavailable there. Concrete types can safely
    // retain the complete compile-time decorator walk; generic roots retain
    // their own decorators and discover concrete nested values through Serde.
    let registered_decorator_calls = if has_generic_parameters {
        &own_decorator_calls
    } else {
        &decorator_calls
    };
    let registered_rust_type_name = if has_generic_parameters {
        quote! {
            concat!(module_path!(), "::", stringify!(#name))
        }
    } else {
        quote! {
            ::std::any::type_name::<#name>()
        }
    };

    // Generate the HasSpytialDecorators implementation
    let expanded = quote! {
        #(#deprecation_shims)*

        const _: () = {
            fn registered_rust_type_name() -> &'static str {
                #registered_rust_type_name
            }

            fn registered_decorators() -> spytial::spytial_annotations::SpytialDecorators {
                spytial::spytial_annotations::SpytialDecoratorsBuilder::new()
                    #(#registered_decorator_calls)*
                    .build()
            }

            spytial::__private::inventory::submit! {
                spytial::spytial_annotations::DecoratorRegistration::new(
                    registered_rust_type_name,
                    #has_generic_parameters,
                    stringify!(#name),
                    #serialized_name,
                    registered_decorators,
                )
            }
        };

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
                    spytial::spytial_annotations::register_type_decorators(
                        #serialized_name,
                        decorators.clone()
                    );
                    spytial::spytial_annotations::register_type_decorators(
                        ::std::any::type_name::<Self>(),
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

/// The attribute as the user wrote it, for spytial-core's `source` block.
///
/// Rendered from the parsed tokens rather than read back from the file:
/// `Span::source_text` covers one token and joining spans is unstable, so the
/// exact bytes are out of reach on the crate's minimum Rust. Token-by-token
/// rendering with conventional spacing gives
/// `#[orientation(selector = "…", directions = ["left", "below"])]`; literals,
/// raw strings and escapes included, come through verbatim, which is the part
/// a reader needs to recognise their own rule in a conflict report.
fn render_attribute(attr: &Attribute) -> String {
    let path = attr
        .path()
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>()
        .join("::");
    match &attr.meta {
        Meta::List(list) => format!("#[{path}({})]", render_tokens(list.tokens.clone())),
        Meta::NameValue(pair) => {
            format!(
                "#[{path} = {}]",
                render_tokens(pair.value.to_token_stream())
            )
        }
        Meta::Path(_) => format!("#[{path}]"),
    }
}

fn render_tokens(tokens: proc_macro2::TokenStream) -> String {
    use proc_macro2::{Delimiter, Spacing, TokenTree};

    let mut out = String::new();
    let mut prev: Option<TokenTree> = None;
    for tree in tokens {
        let space_before = match (&prev, &tree) {
            (None, _) => false,
            // `a, b` not `a , b`
            (_, TokenTree::Punct(p)) if p.as_char() == ',' => false,
            // `line_style(` not `line_style (`
            (Some(TokenTree::Ident(_)), TokenTree::Group(g))
                if g.delimiter() == Delimiter::Parenthesis =>
            {
                false
            }
            // multi-char operators stay together
            (Some(TokenTree::Punct(p)), _) if p.spacing() == Spacing::Joint => false,
            _ => true,
        };
        if space_before {
            out.push(' ');
        }
        match &tree {
            TokenTree::Group(group) => {
                let (open, close) = match group.delimiter() {
                    Delimiter::Parenthesis => ("(", ")"),
                    Delimiter::Bracket => ("[", "]"),
                    Delimiter::Brace => ("{", "}"),
                    Delimiter::None => ("", ""),
                };
                out.push_str(open);
                out.push_str(&render_tokens(group.stream()));
                out.push_str(close);
            }
            other => out.push_str(&other.to_string()),
        }
        prev = Some(tree);
    }
    out
}

#[derive(Debug)]
enum SpatialAttribute {
    Attribute {
        field: String,
        selector: Option<String>,
        filter: Option<String>,
        text_style: Option<TextStyleTok>,
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
        add_edge: Option<AddEdgeTok>,
        show_label: Option<bool>,
        text_style: Option<TextStyleTok>,
        negated: bool,
    },
    AtomColor {
        selector: String,
        value: String,
    },
    AtomStyle {
        selector: Option<String>,
        fill_style: Option<FillStyleTok>,
        border_style: Option<BorderStyleTok>,
        icon_style: Option<IconStyleTok>,
        text_style: Option<TextStyleTok>,
        show_label: Option<bool>,
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
    /// Legacy flat edge form (`value`/`style`/`weight`); desugars at runtime
    /// via the builder's `edge_color`.
    EdgeStyleLegacy {
        field: String,
        value: String,
        selector: Option<String>,
        filter: Option<String>,
        style: Option<String>,
        weight: Option<f64>,
        show_label: Option<bool>,
        hidden: Option<bool>,
    },
    /// spytial-core 3.x block form.
    EdgeStyle {
        field: String,
        selector: Option<String>,
        filter: Option<String>,
        line_style: Option<LineStyleTok>,
        text_style: Option<TextStyleTok>,
        show_label: Option<bool>,
        hidden: Option<bool>,
    },
    HideField {
        field: String,
        selector: Option<String>,
        filter: Option<String>,
    },
    HideAtom {
        selector: String,
    },
    InferredEdge {
        name: String,
        selector: String,
        draw: Option<DrawTok>,
        line_style: Option<LineStyleTok>,
        text_style: Option<TextStyleTok>,
    },
    Tag {
        to_tag: String,
        name: String,
        value: String,
        text_style: Option<TextStyleTok>,
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
    } else if path.is_ident("atom_style") {
        parse_atom_style_args(attr)
    } else if path.is_ident("size") {
        parse_size_args(attr)
    } else if path.is_ident("icon") {
        parse_icon_args(attr)
    } else if path.is_ident("edge_style") {
        parse_edge_style_args(attr)
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

/// The deprecation that applies to this attribute *as written*, and the key
/// that selected it.
///
/// A shape-scoped entry only fires when one of its keys is actually present.
/// That is the whole point: `#[edge_style(value = ...)]` is the deprecated
/// shape and `#[edge_style(field = ..., line_style(...))]` is the current one,
/// so keying the warning on the attribute name alone would condemn both. (The
/// field-based `#[group]` used to be the other example; spytial-core 5.1.0
/// removed it from the language, so it is now a parse error, not a warning.)
/// Keys are read from the
/// group-stripped token string for the same reason the parsers do — a `value`
/// inside `line_style(...)` is not the legacy top-level `value`.
fn deprecation_for(
    attr: &Attribute,
    attr_name: &str,
) -> Option<(&'static DeprecationSpec, Option<&'static str>)> {
    let stripped = attr
        .meta
        .require_list()
        .ok()
        .map(|meta| strip_groups(&meta.tokens.to_string()));

    spec_tables::DEPRECATIONS.iter().find_map(|dep| {
        if dep.attr != attr_name {
            return None;
        }
        if dep.when_any_key.is_empty() {
            return Some((dep, None));
        }
        let tokens = stripped.as_deref()?;
        dep.when_any_key
            .iter()
            .find(|key| has_key(tokens, key))
            .map(|key| (dep, Some(*key)))
    })
}

/// An item that makes rustc emit a deprecation warning pointing at `attr`.
///
/// Proc macros cannot raise warnings directly on stable, so the expansion
/// carries a `#[deprecated]` type and immediately uses it. Every token is
/// spanned to the user's attribute, which is what puts the diagnostic on their
/// `#[icon(...)]` rather than somewhere inside generated code.
///
/// The marker's name is part of the diagnostic — rustc prints "use of
/// deprecated struct `_::icon_is_deprecated`" ahead of the note — so it is
/// spelled to read as a sentence. Each shim gets its own anonymous const, so
/// two identical deprecated attributes on one struct do not collide.
fn deprecation_shim(attr: &Attribute, attr_name: &str) -> Option<proc_macro2::TokenStream> {
    let (dep, matched) = deprecation_for(attr, attr_name)?;

    let form = match matched {
        Some(key) => format!("`#[{}({key} = ...)]`", dep.attr),
        None => format!("`#[{}]`", dep.attr),
    };
    // A deprecated *shape* is replaced by another shape of the same attribute,
    // so "use `#[group]`" would read as a no-op. Name the form instead.
    let replacement = if dep.attr == dep.replaced_by {
        format!("use the current form of `#[{}]`", dep.replaced_by)
    } else {
        format!("use `#[{}]`", dep.replaced_by)
    };
    let note = format!(
        "{form} is deprecated in spytial-core {}; {replacement}. {}",
        spec_tables::SPYTIAL_CORE_VERSION,
        dep.note,
    );

    let span = attr.span();
    let marker = syn::Ident::new(
        &match matched {
            Some(key) => format!("{}_{key}_form_is_deprecated", dep.attr),
            None => format!("{}_is_deprecated", dep.attr),
        },
        span,
    );
    Some(quote_spanned! {span=>
        const _: () = {
            #[deprecated(note = #note)]
            #[allow(non_camel_case_types)]
            struct #marker;
            // The use site. `allow(dead_code)` because nothing calls it — the
            // reference in the signature is the entire point.
            #[allow(dead_code)]
            fn probe(_: #marker) {}
        };
    })
}

/// The generated spec for `attr_name`.
///
/// Every authoring attribute has one — the derive's `attributes(...)` list and
/// the table are generated from the same manifest — so a miss is a bug in the
/// macro rather than in user code.
fn spec_for(attr_name: &str) -> &'static spec_tables::AttrSpec {
    spec_tables::attr_spec(attr_name).unwrap_or_else(|| {
        panic!("no generated spec for #[{attr_name}]; regenerate spec_tables.rs")
    })
}

/// Explain what spytial-core does with a value the macro just rejected.
///
/// The macro rejects either way; this only tells the reader whether they were
/// heading for a hard parse failure downstream or for the much worse outcome —
/// a diagram that renders, silently missing what they asked for.
fn enforcement_note(rule: &FieldRule) -> &'static str {
    match rule.enforcement {
        Some(spec_tables::enforcement::PARSE_ERROR) => " (spytial-core rejects the spec outright)",
        Some(spec_tables::enforcement::VALUE_IGNORED) => {
            " (spytial-core would silently drop it and use the default)"
        }
        Some(spec_tables::enforcement::UNCHECKED) => {
            " (spytial-core would accept it silently; it would simply match nothing)"
        }
        _ => "",
    }
}

/// Check a scalar against its generated closed vocabulary, if it has one.
///
/// `context` names where the key was written, for the error message: an
/// attribute reads `#[align(...)]`, a style block reads `line_style(...)`.
fn check_vocabulary(
    attr: &Attribute,
    context: &str,
    rule: &FieldRule,
    value: &str,
) -> Result<(), syn::Error> {
    let Some(values) = rule.values else {
        return Ok(());
    };
    if values.contains(&value) {
        return Ok(());
    }
    Err(err(
        attr,
        format!(
            "invalid `{}` in {}: {:?}; expected one of: {}{}",
            rule.key,
            context,
            value,
            values.join(", "),
            enforcement_note(rule),
        ),
    ))
}

/// Check an attribute's `direction` against its generated vocabulary.
fn check_direction(attr: &Attribute, attr_name: &str, value: &str) -> Result<(), syn::Error> {
    let rule = spec_for(attr_name).rule("direction").unwrap_or_else(|| {
        panic!("#[{attr_name}] has no direction rule; regenerate spec_tables.rs")
    });
    check_vocabulary(attr, &format!("#[{attr_name}(...)]"), rule, value)
}

/// Check one of an attribute's string keys against its generated vocabulary.
fn check_attr_str(
    attr: &Attribute,
    attr_name: &str,
    key: &str,
    value: &str,
) -> Result<(), syn::Error> {
    let Some(rule) = spec_for(attr_name).rule(key) else {
        return Ok(());
    };
    check_vocabulary(attr, &format!("#[{attr_name}(...)]"), rule, value)
}

/// Check one of an attribute's numeric keys against its generated bounds.
fn check_attr_num(
    attr: &Attribute,
    attr_name: &str,
    key: &str,
    value: f64,
) -> Result<(), syn::Error> {
    let Some(rule) = spec_for(attr_name).rule(key) else {
        return Ok(());
    };
    check_bounds(attr, &format!("#[{attr_name}(...)]"), rule, value)
}

/// The boolean value spytial-core assumes when a field is absent, per the
/// manifest. Defaults are carried as text in the tables whatever their JSON
/// type, so this parses it back.
fn default_bool(attr_name: &str, key: &str) -> bool {
    match default_for(attr_name, key) {
        "true" => true,
        "false" => false,
        other => panic!(
            "#[{attr_name}]'s `{key}` default is {other:?}, not a boolean; \
             regenerate spec_tables.rs"
        ),
    }
}

/// The value spytial-core assumes when a field is absent, per the manifest.
///
/// Only called for fields the manifest actually gives a default; a miss means
/// the language changed and the caller's fallback needs revisiting.
fn default_for(attr_name: &str, key: &str) -> &'static str {
    spec_for(attr_name)
        .rule(key)
        .and_then(|r| r.default)
        .unwrap_or_else(|| {
            panic!("#[{attr_name}]'s `{key}` has no manifest default; regenerate spec_tables.rs")
        })
}

/// Check a number against its generated bounds.
fn check_bounds(
    attr: &Attribute,
    context: &str,
    rule: &FieldRule,
    value: f64,
) -> Result<(), syn::Error> {
    let fail = |expected: String| {
        Err(err(
            attr,
            format!(
                "invalid `{}` in {}: {}; {}{}",
                rule.key,
                context,
                value,
                expected,
                enforcement_note(rule),
            ),
        ))
    };
    if !value.is_finite() {
        return fail("must be a finite number".to_string());
    }
    if let Some(x) = rule.exclusive_min {
        if value <= x {
            return fail(format!("must be greater than {x}"));
        }
    }
    if let Some(m) = rule.min {
        if value < m {
            return fail(format!("must be at least {m}"));
        }
    }
    if let Some(m) = rule.max {
        if value > m {
            return fail(format!("must be at most {m}"));
        }
    }
    Ok(())
}

/// Walk the meta items of `attr` and emit a `syn::Error` (pointing at the
/// offending key's span) for any key the generated spec doesn't list.  This
/// catches typos like `#[orientation(typo = "...")]` at compile time instead of
/// silently falling back to defaults.
///
/// The value side of each pair is consumed but not interpreted; the existing
/// string-based extractors handle the actual value parsing.
fn validate_known_keys(attr: &Attribute, attr_name: &str) -> Result<(), syn::Error> {
    let known = spec_for(attr_name).keys;

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
        } else if meta.input.peek(syn::token::Paren) {
            // Group-valued key like `line_style(color = "red")`: consume the
            // parenthesized block so parse_nested_meta can advance. The block's
            // contents are parsed by the style-block extractors.
            let content;
            syn::parenthesized!(content in meta.input);
            let _ = content.parse::<proc_macro2::TokenStream>();
        }
        Ok(())
    })
}

fn parse_attribute_args(attr: &Attribute) -> Result<Option<SpatialAttribute>, syn::Error> {
    validate_known_keys(attr, "attribute")?;
    // Look for `field = "..."`; fall back to `name` when it's omitted.
    if let Ok(meta) = attr.meta.require_list() {
        let tokens = &meta.tokens;
        let token_str = tokens.to_string();
        let stripped = strip_groups(&token_str);
        let text_style = parse_text_style_group(attr, &token_str)?;

        return Ok(Some(SpatialAttribute::Attribute {
            field: extract_string_from_tokens(&stripped, "field")
                .unwrap_or_else(|| "name".to_string()),
            selector: extract_string_from_tokens(&stripped, "selector"),
            filter: extract_string_from_tokens(&stripped, "filter"),
            text_style,
        }));
    }

    Ok(Some(SpatialAttribute::Attribute {
        field: "name".to_string(),
        selector: None,
        filter: None,
        text_style: None,
    }))
}

fn parse_flag_args(attr: &Attribute) -> Result<Option<SpatialAttribute>, syn::Error> {
    validate_known_keys(attr, "flag")?;
    let name = attr
        .meta
        .require_list()
        .ok()
        .and_then(|meta| extract_string_from_tokens(&meta.tokens.to_string(), "name"));

    // `flag` is a closed two-value vocabulary with no default. The old fallback
    // here was "important", which spytial-core has never recognized — a bare
    // `#[flag]` produced a directive the engine silently dropped. Requiring the
    // name is the only honest option.
    let Some(name) = name else {
        let rule = spec_for("flag")
            .rule("name")
            .expect("flag spec has no name rule; regenerate spec_tables.rs");
        return Err(err(
            attr,
            format!(
                "#[flag] needs a name: expected one of: {}",
                rule.values.unwrap_or(&[]).join(", "),
            ),
        ));
    };

    let rule = spec_for("flag")
        .rule("name")
        .expect("flag spec has no name rule; regenerate spec_tables.rs");
    check_vocabulary(attr, "#[flag(...)]", rule, &name)?;
    Ok(Some(SpatialAttribute::Flag { name }))
}

/// Check a `directions` list against the generated vocabulary and list rules.
///
/// spytial-core rejects a contradictory set at parse time (`above` with
/// `below`, or a `directly*` variant alongside anything but its own plain
/// counterpart) but silently accepts a value outside the vocabulary, which then
/// matches nothing. Both fail here.
fn check_directions(attr: &Attribute, directions: &[String]) -> Result<(), syn::Error> {
    let rule = spec_for("orientation")
        .rule("directions")
        .expect("orientation spec has no directions rule; regenerate spec_tables.rs");

    if directions.is_empty() {
        return Err(err(
            attr,
            format!(
                "#[orientation(...)] needs at least one direction: expected one or more of: {}",
                rule.values.unwrap_or(&[]).join(", "),
            ),
        ));
    }

    for d in directions {
        check_vocabulary(attr, "#[orientation(...)]", rule, d)?;
    }

    for pair in spec_tables::ORIENTATION_AT_MOST_ONE_OF {
        let present: Vec<&str> = pair
            .iter()
            .copied()
            .filter(|v| directions.iter().any(|d| d == v))
            .collect();
        if present.len() > 1 {
            return Err(err(
                attr,
                format!(
                    "contradictory directions in #[orientation(...)]: {}; at most one of {} may be given",
                    present.join(" and "),
                    pair.join(", "),
                ),
            ));
        }
    }

    for (direct, allowed) in spec_tables::ORIENTATION_NARROWS_TO {
        if !directions.iter().any(|d| d == direct) {
            continue;
        }
        if let Some(bad) = directions.iter().find(|d| !allowed.contains(&d.as_str())) {
            return Err(err(
                attr,
                format!(
                    "`{direct}` in #[orientation(...)] cannot be combined with `{bad}`; \
                     alongside `{direct}` the only other value allowed is `{}`",
                    allowed
                        .iter()
                        .find(|a| *a != direct)
                        .copied()
                        .unwrap_or(direct),
                ),
            ));
        }
    }

    Ok(())
}

fn parse_orientation_args(attr: &Attribute) -> Result<Option<SpatialAttribute>, syn::Error> {
    validate_known_keys(attr, "orientation")?;
    if let Ok(meta) = attr.meta.require_list() {
        let tokens = &meta.tokens;
        let token_str = tokens.to_string();

        let selector = extract_string_from_tokens(&token_str, "selector").unwrap_or_default();
        // No default: `directions` is required, and the old fallback of
        // ["up", "down"] was outside the vocabulary entirely, so it produced a
        // constraint that matched nothing.
        let directions = extract_array_from_tokens(&token_str, "directions").unwrap_or_default();
        check_directions(attr, &directions)?;
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
    // The field-based shape (`field`/`group_on`/`add_to_group`) left the
    // language in spytial-core 5.1.0. Its keys are gone from the generated spec,
    // so `validate_known_keys` would already refuse them — but as "unknown
    // parameter `field`", which reads like a typo. Catch the shape first so the
    // error says what replaced it.
    if let Ok(meta) = attr.meta.require_list() {
        if has_key(&strip_groups(&meta.tokens.to_string()), "field") {
            return Err(err(
                attr,
                format!(
                    "`#[group(field = ...)]` was removed from the layout-spec language in \
                     spytial-core 5.1.0 (this crate vendors {}); write the selector form. A \
                     binary selector whose first column is the group key and whose second is \
                     the members says the same thing without tuple indices: over \
                     `works_in: Employee -> Department`, `field = \"works_in\", group_on = 1, \
                     add_to_group = 0` becomes `selector = \"~works_in\", name = \"...\"`.",
                    spec_tables::SPYTIAL_CORE_VERSION,
                ),
            ));
        }
    }
    validate_known_keys(attr, "group")?;
    if let Ok(meta) = attr.meta.require_list() {
        let tokens = &meta.tokens;
        let token_str = tokens.to_string();
        // Flat keys are read from the group-stripped string so nothing inside
        // add_edge(...)/text_style(...) is mistaken for a top-level key.
        let stripped = strip_groups(&token_str);
        let negated = extract_bool_from_tokens(&stripped, "negated").unwrap_or(false);
        let selector = extract_string_from_tokens(&stripped, "selector").unwrap_or_default();
        let name =
            extract_string_from_tokens(&stripped, "name").unwrap_or_else(|| "default".to_string());
        let add_edge = parse_add_edge(attr, &token_str, &stripped)?;
        let show_label = extract_bool_from_tokens(&stripped, "show_label");
        let text_style = parse_text_style_group(attr, &token_str)?;

        Ok(Some(SpatialAttribute::GroupSelector {
            selector,
            name,
            add_edge,
            show_label,
            text_style,
            negated,
        }))
    } else {
        Ok(None)
    }
}

fn parse_align_args(attr: &Attribute) -> Result<Option<SpatialAttribute>, syn::Error> {
    validate_known_keys(attr, "align")?;
    if let Ok(meta) = attr.meta.require_list() {
        let tokens = &meta.tokens;
        let token_str = tokens.to_string();

        let selector = extract_string_from_tokens(&token_str, "selector").unwrap_or_default();
        let direction = extract_string_from_tokens(&token_str, "direction")
            .unwrap_or_else(|| "horizontal".to_string());
        check_direction(attr, "align", &direction)?;
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
    validate_known_keys(attr, "cyclic")?;
    if let Ok(meta) = attr.meta.require_list() {
        let tokens = &meta.tokens;
        let token_str = tokens.to_string();

        let selector = extract_string_from_tokens(&token_str, "selector").unwrap_or_default();
        // The manifest's own default. The old fallback here was "up", which is
        // not a cycle direction at all — spytial-core would accept it and lay
        // out clockwise regardless.
        let direction = extract_string_from_tokens(&token_str, "direction")
            .unwrap_or_else(|| default_for("cyclic", "direction").to_string());
        check_direction(attr, "cyclic", &direction)?;
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
    validate_known_keys(attr, "atom_color")?;
    if let Ok(meta) = attr.meta.require_list() {
        let tokens = &meta.tokens;
        let token_str = tokens.to_string();

        let selector = extract_string_from_tokens(&token_str, "selector").unwrap_or_default();
        let value =
            extract_string_from_tokens(&token_str, "value").unwrap_or_else(|| "blue".to_string());

        Ok(Some(SpatialAttribute::AtomColor { selector, value }))
    } else {
        Ok(None)
    }
}

fn parse_size_args(attr: &Attribute) -> Result<Option<SpatialAttribute>, syn::Error> {
    validate_known_keys(attr, "size")?;
    if let Ok(meta) = attr.meta.require_list() {
        let tokens = &meta.tokens;
        let token_str = tokens.to_string();

        let selector = extract_string_from_tokens(&token_str, "selector").unwrap_or_default();
        let height = extract_number_from_tokens(&token_str, "height").unwrap_or(20);
        let width = extract_number_from_tokens(&token_str, "width").unwrap_or(30);
        // Both are `exclusiveMinimum: 0` upstream; a zero-sized atom is a
        // parse error there, so catch it here.
        check_attr_num(attr, "size", "height", height as f64)?;
        check_attr_num(attr, "size", "width", width as f64)?;

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
    validate_known_keys(attr, "icon")?;
    if let Ok(meta) = attr.meta.require_list() {
        let tokens = &meta.tokens;
        let token_str = tokens.to_string();

        let selector = extract_string_from_tokens(&token_str, "selector").unwrap_or_default();
        let path = extract_string_from_tokens(&token_str, "path")
            .unwrap_or_else(|| "icon.png".to_string());
        // The manifest's default is `false`, not `true`. Getting this backwards
        // inverted the whole deprecation rewrite for a bare `#[icon]`: it drew a
        // corner badge with the label on, where the engine draws a full-box icon
        // with the label off.
        let show_labels = extract_bool_from_tokens(&token_str, "show_labels")
            .unwrap_or_else(|| default_bool("icon", "show_labels"));

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
    validate_known_keys(attr, "edge_style")?;
    if let Ok(meta) = attr.meta.require_list() {
        let tokens = &meta.tokens;
        let token_str = tokens.to_string();
        // Flat keys come from the group-stripped string, so `weight`/`color`
        // inside `line_style(...)` are never read as legacy flat keys.
        let stripped = strip_groups(&token_str);

        let field = extract_string_from_tokens(&stripped, "field")
            .unwrap_or_else(|| "relation".to_string());
        let selector = extract_string_from_tokens(&stripped, "selector");
        let filter = extract_string_from_tokens(&stripped, "filter");
        let show_label = extract_bool_from_tokens(&stripped, "show_label");
        let hidden = extract_bool_from_tokens(&stripped, "hidden");

        let line_style = parse_line_style_group(attr, &token_str)?;
        let text_style = parse_text_style_group(attr, &token_str)?;

        // Legacy flat keys (2.x edgeColor shape). They carry the same generated
        // rules as their block replacements, so check them the same way — a
        // `style = "dashd"` typo used to reach the runtime, which dropped the
        // pattern with a note on stderr, while the identical typo written as
        // `line_style(pattern = "dashd")` was a compile error.
        let value = extract_string_from_tokens(&stripped, "value");
        let style = extract_string_from_tokens(&stripped, "style");
        let weight = extract_float_from_tokens(&stripped, "weight");
        if let Some(style) = &style {
            // Checked against the normalized form, because that is what the
            // legacy path accepts: spytial-core's `normalizeEdgeStyle` trims and
            // lowercases, so `"Dotted"` is a valid 2.x spelling. The block form
            // gets no such leniency — `line_style(pattern = ...)` is matched
            // exactly. Only a value that survives neither, like `"dashd"`, fails.
            check_attr_str(
                attr,
                "edge_style",
                "style",
                style.trim().to_ascii_lowercase().as_str(),
            )?;
        }
        if let Some(weight) = weight {
            check_attr_num(attr, "edge_style", "weight", weight)?;
        }

        let has_legacy = value.is_some() || style.is_some() || weight.is_some();
        let has_blocks = line_style.is_some() || text_style.is_some();
        if has_legacy && has_blocks {
            return Err(err(
                attr,
                "edge_style got both the legacy flat keys (value/style/weight) and \
                 line_style(...)/text_style(...) blocks; use the blocks only"
                    .to_string(),
            ));
        }

        // An attribute carrying no styling at all is a 0.1-era flat form: the
        // old parser defaulted `value` to "blue", so a bare
        // `#[edge_style(field = "left")]` (or one with only show_label/hidden)
        // must keep drawing a blue edge. Writing a block is what opts into the
        // new no-default semantics.
        if has_legacy || !has_blocks {
            return Ok(Some(SpatialAttribute::EdgeStyleLegacy {
                field,
                value: value.unwrap_or_else(|| "blue".to_string()),
                selector,
                filter,
                style,
                weight,
                show_label,
                hidden,
            }));
        }

        Ok(Some(SpatialAttribute::EdgeStyle {
            field,
            selector,
            filter,
            line_style,
            text_style,
            show_label,
            hidden,
        }))
    } else {
        Ok(None)
    }
}

fn parse_atom_style_args(attr: &Attribute) -> Result<Option<SpatialAttribute>, syn::Error> {
    validate_known_keys(attr, "atom_style")?;
    if let Ok(meta) = attr.meta.require_list() {
        let tokens = &meta.tokens;
        let token_str = tokens.to_string();
        let stripped = strip_groups(&token_str);

        Ok(Some(SpatialAttribute::AtomStyle {
            selector: extract_string_from_tokens(&stripped, "selector"),
            fill_style: parse_fill_style_group(attr, &token_str)?,
            border_style: parse_border_style_group(attr, &token_str)?,
            icon_style: parse_icon_style_group(attr, &token_str)?,
            text_style: parse_text_style_group(attr, &token_str)?,
            show_label: extract_bool_from_tokens(&stripped, "show_label"),
        }))
    } else {
        Ok(None)
    }
}

fn parse_hide_field_args(attr: &Attribute) -> Result<Option<SpatialAttribute>, syn::Error> {
    validate_known_keys(attr, "hide_field")?;
    if let Ok(meta) = attr.meta.require_list() {
        let tokens = &meta.tokens;
        let token_str = tokens.to_string();

        let field =
            extract_string_from_tokens(&token_str, "field").unwrap_or_else(|| "field".to_string());
        let selector = extract_string_from_tokens(&token_str, "selector");
        let filter = extract_string_from_tokens(&token_str, "filter");

        Ok(Some(SpatialAttribute::HideField {
            field,
            selector,
            filter,
        }))
    } else {
        Ok(None)
    }
}

fn parse_hide_atom_args(attr: &Attribute) -> Result<Option<SpatialAttribute>, syn::Error> {
    validate_known_keys(attr, "hide_atom")?;
    if let Ok(meta) = attr.meta.require_list() {
        let tokens = &meta.tokens;
        let token_str = tokens.to_string();

        let selector = extract_string_from_tokens(&token_str, "selector").unwrap_or_default();

        Ok(Some(SpatialAttribute::HideAtom { selector }))
    } else {
        Ok(None)
    }
}

fn parse_inferred_edge_args(attr: &Attribute) -> Result<Option<SpatialAttribute>, syn::Error> {
    validate_known_keys(attr, "inferred_edge")?;
    if let Ok(meta) = attr.meta.require_list() {
        let tokens = &meta.tokens;
        let token_str = tokens.to_string();
        let stripped = strip_groups(&token_str);

        let name =
            extract_string_from_tokens(&stripped, "name").unwrap_or_else(|| "edge".to_string());
        let selector = extract_string_from_tokens(&stripped, "selector").unwrap_or_default();

        Ok(Some(SpatialAttribute::InferredEdge {
            name,
            selector,
            draw: parse_draw(attr, &stripped)?,
            line_style: parse_line_style_group(attr, &token_str)?,
            text_style: parse_text_style_group(attr, &token_str)?,
        }))
    } else {
        Ok(None)
    }
}

fn parse_tag_args(attr: &Attribute) -> Result<Option<SpatialAttribute>, syn::Error> {
    validate_known_keys(attr, "tag")?;
    if let Ok(meta) = attr.meta.require_list() {
        let tokens = &meta.tokens;
        let token_str = tokens.to_string();
        let stripped = strip_groups(&token_str);

        let to_tag = extract_string_from_tokens(&stripped, "to_tag").unwrap_or_default();
        let name = extract_string_from_tokens(&stripped, "name").unwrap_or_default();
        let value = extract_string_from_tokens(&stripped, "value").unwrap_or_default();

        Ok(Some(SpatialAttribute::Tag {
            to_tag,
            name,
            value,
            text_style: parse_text_style_group(attr, &token_str)?,
        }))
    } else {
        Ok(None)
    }
}

// ---------------------------------------------------------------------------
// Style blocks (spytial-core 3.x): parsed attribute forms + codegen
//
// The nested-group attribute syntax mirrors the YAML blocks 1:1:
//   line_style(color = "red", pattern = "dashed", weight = 2.0, highlight = "...")
//   text_style(size = "small", color = "gray")
//   border_style(color = "steelblue", width = 2.0) / fill_style(color = "#eef6ff")
//   add_edge(points = "togroup", line_style(...), text_style(...))
// Closed vocabularies (pattern/size/points) and weight positivity are checked
// here, at compile time — spytial-core silently drops invalid leaves, so the
// macro is where a typo must fail.
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
struct LineStyleTok {
    color: Option<String>,
    pattern: Option<String>,
    weight: Option<f64>,
    highlight: Option<String>,
}

#[derive(Debug, Default)]
struct TextStyleTok {
    size: Option<String>,
    color: Option<String>,
}

#[derive(Debug, Default)]
struct BorderStyleTok {
    color: Option<String>,
    width: Option<f64>,
}

#[derive(Debug, Default)]
struct FillStyleTok {
    color: Option<String>,
}

#[derive(Debug, Default)]
struct IconStyleTok {
    path: Option<String>,
    placement: Option<String>,
    opacity: Option<f64>,
}

/// A parsed `inferred_edge` `draw = "<end> -> <end>"` value. Each end is
/// `None` (written `_`: the atom itself) or a group-constraint name.
#[derive(Debug)]
struct DrawTok {
    source: Option<String>,
    target: Option<String>,
}

#[derive(Debug)]
enum AddEdgeTok {
    /// Bare direction form: `add_edge = "togroup"`.
    Direction(String),
    /// Block form: `add_edge(points = "...", line_style(...), text_style(...))`.
    Block {
        points: String,
        line_style: Option<LineStyleTok>,
        text_style: Option<TextStyleTok>,
    },
}

fn err(attr: &Attribute, msg: String) -> syn::Error {
    syn::Error::new_spanned(attr, msg)
}

/// The generated spec for a style block.
///
/// Like [`spec_for`], a miss is a macro bug: the block parsers below and the
/// table are both derived from the manifest's `blocks`.
fn block_for(block: &str) -> &'static spec_tables::BlockSpec {
    spec_tables::block_spec(block)
        .unwrap_or_else(|| panic!("no generated spec for {block}(...); regenerate spec_tables.rs"))
}

/// Validate one string leaf of a style block against its generated rule.
fn check_block_str(
    attr: &Attribute,
    block: &str,
    leaf: &str,
    value: &Option<String>,
) -> Result<(), syn::Error> {
    let (Some(value), Some(rule)) = (value, block_for(block).rule(leaf)) else {
        return Ok(());
    };
    check_vocabulary(attr, &format!("{block}(...)"), rule, value)
}

/// Validate one numeric leaf of a style block against its generated rule.
fn check_block_num(
    attr: &Attribute,
    block: &str,
    leaf: &str,
    value: Option<f64>,
) -> Result<(), syn::Error> {
    let (Some(value), Some(rule)) = (value, block_for(block).rule(leaf)) else {
        return Ok(());
    };
    check_bounds(attr, &format!("{block}(...)"), rule, value)
}

/// The value spytial-core assumes when a block leaf is absent, per the manifest.
fn default_for_block(block: &str, leaf: &str) -> &'static str {
    block_for(block)
        .rule(leaf)
        .and_then(|r| r.default)
        .unwrap_or_else(|| {
            panic!("{block}(...)'s `{leaf}` has no manifest default; regenerate spec_tables.rs")
        })
}

/// Reject any leaf inside a style block that the generated spec doesn't list,
/// so `line_style(colour = "red")` fails here instead of rendering unstyled.
fn validate_block_leaves(attr: &Attribute, block: &str, body: &str) -> Result<(), syn::Error> {
    let known = block_for(block).rules;
    for leaf in top_level_keys(body) {
        if !known.iter().any(|r| r.key == leaf) {
            return Err(err(
                attr,
                format!(
                    "unknown leaf `{}` in {}(...); expected one of: {}",
                    leaf,
                    block,
                    known.iter().map(|r| r.key).collect::<Vec<_>>().join(", "),
                ),
            ));
        }
    }
    Ok(())
}

/// Parse a `line_style(...)` group out of `tokens`, validating its leaves.
fn parse_line_style_group(
    attr: &Attribute,
    tokens: &str,
) -> Result<Option<LineStyleTok>, syn::Error> {
    let Some(body) = extract_group_from_tokens(tokens, "line_style") else {
        return Ok(None);
    };
    validate_block_leaves(attr, "line_style", &body)?;
    let ls = LineStyleTok {
        color: extract_string_from_tokens(&body, "color"),
        pattern: extract_string_from_tokens(&body, "pattern"),
        weight: extract_float_from_tokens(&body, "weight"),
        highlight: extract_string_from_tokens(&body, "highlight"),
    };
    check_block_str(attr, "line_style", "pattern", &ls.pattern)?;
    check_block_num(attr, "line_style", "weight", ls.weight)?;
    Ok(Some(ls))
}

/// Parse a `text_style(...)` group out of `tokens`, validating its leaves.
fn parse_text_style_group(
    attr: &Attribute,
    tokens: &str,
) -> Result<Option<TextStyleTok>, syn::Error> {
    let Some(body) = extract_group_from_tokens(tokens, "text_style") else {
        return Ok(None);
    };
    validate_block_leaves(attr, "text_style", &body)?;
    let ts = TextStyleTok {
        size: extract_string_from_tokens(&body, "size"),
        color: extract_string_from_tokens(&body, "color"),
    };
    check_block_str(attr, "text_style", "size", &ts.size)?;
    Ok(Some(ts))
}

/// Parse a `border_style(...)` group out of `tokens`, validating its leaves.
fn parse_border_style_group(
    attr: &Attribute,
    tokens: &str,
) -> Result<Option<BorderStyleTok>, syn::Error> {
    let Some(body) = extract_group_from_tokens(tokens, "border_style") else {
        return Ok(None);
    };
    validate_block_leaves(attr, "border_style", &body)?;
    let bs = BorderStyleTok {
        color: extract_string_from_tokens(&body, "color"),
        width: extract_float_from_tokens(&body, "width"),
    };
    check_block_num(attr, "border_style", "width", bs.width)?;
    Ok(Some(bs))
}

/// Parse a `fill_style(...)` group out of `tokens`.
fn parse_fill_style_group(
    attr: &Attribute,
    tokens: &str,
) -> Result<Option<FillStyleTok>, syn::Error> {
    let Some(body) = extract_group_from_tokens(tokens, "fill_style") else {
        return Ok(None);
    };
    validate_block_leaves(attr, "fill_style", &body)?;
    Ok(Some(FillStyleTok {
        color: extract_string_from_tokens(&body, "color"),
    }))
}

/// Parse an `icon_style(...)` group out of `tokens`, validating its leaves.
///
/// spytial-core 4.2 split the old `icon` directive's single `showLabels` boolean
/// into an independent icon block and an `atom_style` `show_label`, which is what
/// makes a faded watermark — or a hidden label with no icon — expressible.
fn parse_icon_style_group(
    attr: &Attribute,
    tokens: &str,
) -> Result<Option<IconStyleTok>, syn::Error> {
    let Some(body) = extract_group_from_tokens(tokens, "icon_style") else {
        return Ok(None);
    };
    validate_block_leaves(attr, "icon_style", &body)?;
    let is = IconStyleTok {
        path: extract_string_from_tokens(&body, "path"),
        placement: extract_string_from_tokens(&body, "placement"),
        opacity: extract_float_from_tokens(&body, "opacity"),
    };
    check_block_str(attr, "icon_style", "placement", &is.placement)?;
    check_block_num(attr, "icon_style", "opacity", is.opacity)?;
    Ok(Some(is))
}

/// Parse an `inferred_edge` `draw` value out of the stripped token string.
///
/// Rejects at compile time everything spytial-core would reject when it parses
/// the spec (no `->`, more than one, an empty end) plus the redundant
/// `_ -> _`, which spytial-core silently drops — so a `draw` that survives the
/// macro is a `draw` that does something.
fn parse_draw(attr: &Attribute, stripped: &str) -> Result<Option<DrawTok>, syn::Error> {
    let Some(raw) = extract_string_from_tokens(stripped, "draw") else {
        return Ok(None);
    };
    let parts: Vec<&str> = raw.split("->").collect();
    if parts.len() != 2 {
        return Err(err(
            attr,
            format!(
                "draw must contain exactly one \"->\" (e.g. \"regions -> regions\" or \"_ -> regions\"); got {raw:?}"
            ),
        ));
    }
    let end = |part: &str| -> Result<Option<String>, syn::Error> {
        match part.trim() {
            "" => Err(err(
                attr,
                format!(
                    "draw has an empty endpoint in {raw:?}; each end must be \"_\" or a group name"
                ),
            )),
            "_" => Ok(None),
            name => Ok(Some(name.to_string())),
        }
    };
    let source = end(parts[0])?;
    let target = end(parts[1])?;
    if source.is_none() && target.is_none() {
        return Err(err(
            attr,
            "draw = \"_ -> _\" is the default (both ends on the tuple's own atoms); drop the draw key"
                .to_string(),
        ));
    }
    Ok(Some(DrawTok { source, target }))
}

/// Parse an `add_edge` value: either the bare string form (from the stripped
/// token string) or the styled block form.
fn parse_add_edge(
    attr: &Attribute,
    tokens: &str,
    stripped: &str,
) -> Result<Option<AddEdgeTok>, syn::Error> {
    if let Some(body) = extract_group_from_tokens(tokens, "add_edge") {
        // The block form is a block like any other: reject unknown leaves, or a
        // typo'd `pointz` silently yields the `none` default, i.e. the connector
        // the user asked for is never drawn.
        validate_block_leaves(attr, "add_edge", &body)?;
        let points = extract_string_from_tokens(&body, "points")
            .unwrap_or_else(|| default_for_block("add_edge", "points").to_string());
        check_block_str(attr, "add_edge", "points", &Some(points.clone()))?;
        return Ok(Some(AddEdgeTok::Block {
            points,
            line_style: parse_line_style_group(attr, &body)?,
            text_style: parse_text_style_group(attr, &body)?,
        }));
    }
    if let Some(direction) = extract_string_from_tokens(stripped, "add_edge") {
        // The bare form carries the same vocabulary, stated on `group`'s own
        // `addEdge` field.
        check_attr_str(attr, "group", "add_edge", &direction)?;
        return Ok(Some(AddEdgeTok::Direction(direction)));
    }
    Ok(None)
}

// Codegen: turn the parsed blocks into runtime constructor tokens.

fn quote_opt_string(v: &Option<String>) -> proc_macro2::TokenStream {
    match v {
        Some(s) => quote! { Some(#s.to_string()) },
        None => quote! { None },
    }
}

/// Quote an optional string as an `Option<&str>` argument, for the builder
/// methods that take borrowed selectors.
fn quote_opt_str_ref(v: &Option<String>) -> proc_macro2::TokenStream {
    match v {
        Some(s) => quote! { Some(#s) },
        None => quote! { None },
    }
}

fn quote_opt_f64(v: Option<f64>) -> proc_macro2::TokenStream {
    match v {
        Some(n) => quote! { Some(#n) },
        None => quote! { None },
    }
}

fn quote_pattern(p: &str) -> proc_macro2::TokenStream {
    match p {
        "solid" => quote! { spytial::spytial_annotations::LinePattern::Solid },
        "dashed" => quote! { spytial::spytial_annotations::LinePattern::Dashed },
        _ => quote! { spytial::spytial_annotations::LinePattern::Dotted },
    }
}

fn quote_size(s: &str) -> proc_macro2::TokenStream {
    match s {
        "small" => quote! { spytial::spytial_annotations::TextSize::Small },
        "large" => quote! { spytial::spytial_annotations::TextSize::Large },
        _ => quote! { spytial::spytial_annotations::TextSize::Normal },
    }
}

fn quote_points(p: &str) -> proc_macro2::TokenStream {
    match p {
        "togroup" => quote! { spytial::spytial_annotations::GroupEdgePoints::Togroup },
        "fromgroup" => quote! { spytial::spytial_annotations::GroupEdgePoints::Fromgroup },
        _ => quote! { spytial::spytial_annotations::GroupEdgePoints::None },
    }
}

fn quote_draw(draw: &Option<DrawTok>) -> proc_macro2::TokenStream {
    let end = |e: &Option<String>| match e {
        Some(name) => {
            quote! { spytial::spytial_annotations::DrawEnd::Group(#name.to_string()) }
        }
        None => quote! { spytial::spytial_annotations::DrawEnd::Atom },
    };
    match draw {
        None => quote! { None },
        Some(d) => {
            let source = end(&d.source);
            let target = end(&d.target);
            quote! {
                Some(spytial::spytial_annotations::InferredEdgeDraw {
                    source: #source,
                    target: #target,
                })
            }
        }
    }
}

fn quote_line_style(ls: &Option<LineStyleTok>) -> proc_macro2::TokenStream {
    match ls {
        None => quote! { None },
        Some(ls) => {
            let color = quote_opt_string(&ls.color);
            let pattern = match &ls.pattern {
                Some(p) => {
                    let tok = quote_pattern(p);
                    quote! { Some(#tok) }
                }
                None => quote! { None },
            };
            let weight = quote_opt_f64(ls.weight);
            let highlight = quote_opt_string(&ls.highlight);
            quote! {
                Some(spytial::spytial_annotations::LineStyle {
                    color: #color,
                    pattern: #pattern,
                    weight: #weight,
                    highlight: #highlight,
                })
            }
        }
    }
}

fn quote_text_style(ts: &Option<TextStyleTok>) -> proc_macro2::TokenStream {
    match ts {
        None => quote! { None },
        Some(ts) => {
            let size = match &ts.size {
                Some(s) => {
                    let tok = quote_size(s);
                    quote! { Some(#tok) }
                }
                None => quote! { None },
            };
            let color = quote_opt_string(&ts.color);
            quote! {
                Some(spytial::spytial_annotations::TextStyle {
                    size: #size,
                    color: #color,
                })
            }
        }
    }
}

fn quote_border_style(bs: &Option<BorderStyleTok>) -> proc_macro2::TokenStream {
    match bs {
        None => quote! { None },
        Some(bs) => {
            let color = quote_opt_string(&bs.color);
            let width = quote_opt_f64(bs.width);
            quote! {
                Some(spytial::spytial_annotations::BorderStyle {
                    color: #color,
                    width: #width,
                })
            }
        }
    }
}

fn quote_placement(p: &str) -> proc_macro2::TokenStream {
    match p {
        "badge" => quote! { spytial::spytial_annotations::IconPlacement::Badge },
        _ => quote! { spytial::spytial_annotations::IconPlacement::Full },
    }
}

fn quote_icon_style(is: &Option<IconStyleTok>) -> proc_macro2::TokenStream {
    match is {
        None => quote! { None },
        Some(is) => {
            let path = quote_opt_string(&is.path);
            let placement = match &is.placement {
                Some(p) => {
                    let tok = quote_placement(p);
                    quote! { Some(#tok) }
                }
                None => quote! { None },
            };
            let opacity = quote_opt_f64(is.opacity);
            quote! {
                Some(spytial::spytial_annotations::IconStyle {
                    path: #path,
                    placement: #placement,
                    opacity: #opacity,
                })
            }
        }
    }
}

fn quote_fill_style(fs: &Option<FillStyleTok>) -> proc_macro2::TokenStream {
    match fs {
        None => quote! { None },
        Some(fs) => {
            let color = quote_opt_string(&fs.color);
            quote! {
                Some(spytial::spytial_annotations::FillStyle { color: #color })
            }
        }
    }
}

fn quote_add_edge(ae: &Option<AddEdgeTok>) -> proc_macro2::TokenStream {
    match ae {
        None => quote! { None },
        Some(AddEdgeTok::Direction(d)) => {
            let tok = quote_points(d);
            quote! { Some(spytial::spytial_annotations::GroupEdgeValue::Direction(#tok)) }
        }
        Some(AddEdgeTok::Block {
            points,
            line_style,
            text_style,
        }) => {
            let points_tok = quote_points(points);
            let ls = quote_line_style(line_style);
            let ts = quote_text_style(text_style);
            quote! {
                Some(spytial::spytial_annotations::GroupEdgeValue::Block(
                    spytial::spytial_annotations::GroupEdge {
                        points: #points_tok,
                        line_style: #ls,
                        text_style: #ts,
                    },
                ))
            }
        }
    }
}

/// Extract the value of a `key = "..."` pair from a token string.
///
/// Both literal shapes work, whatever the spacing around `=`:
/// `"{x : Node | @:(x.color) = \"Red\"}"` and the raw
/// `r#"{x : Node | @:(x.color) = "Red"}"#`. simple-graph-query 3.0 wants string
/// comparands quoted, so selectors carry quotes either escaped or raw.
///
/// The whole literal's source text goes to `syn`, which decodes it exactly as
/// rustc would. `key` matches only on an identifier boundary and only outside
/// literals, so neither a longer key that ends in `key` nor `key = "..."` text
/// *inside* a selector can be mistaken for the pair.
/// Where the value of each `key = <value>` pair starts, in order.
///
/// Every key extractor scans through this, so they all agree on what counts as
/// a key: matched on an identifier boundary (`line_style` never answers for
/// `style`) and only outside string literals, so `width = 3` written *inside* a
/// selector is that selector's content rather than a `width` of 3. Any spacing
/// around `=` works, newlines included.
fn key_value_starts(chars: &[char], key: &str) -> Vec<usize> {
    let key_chars: Vec<char> = key.chars().collect();
    let mut starts = Vec::new();
    let mut i = 0usize;
    while i < chars.len() {
        if let Some(end) = string_literal_end(chars, i) {
            i = end;
            continue;
        }
        if chars[i..].starts_with(&key_chars[..])
            && (i == 0 || !(chars[i - 1].is_alphanumeric() || chars[i - 1] == '_'))
        {
            let mut j = i + key_chars.len();
            // Identifier must end exactly at the key.
            if j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '_') {
                i += 1;
                continue;
            }
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            if chars.get(j) == Some(&'=') {
                j += 1;
                while j < chars.len() && chars[j].is_whitespace() {
                    j += 1;
                }
                starts.push(j);
                i = j;
                continue;
            }
        }
        i += 1;
    }
    starts
}

/// Whether `key` appears as a key at all — used to pick between an attribute's
/// two shapes (`group`'s field-based vs selector-based) without a substring
/// test that a selector could satisfy by accident.
fn has_key(tokens: &str, key: &str) -> bool {
    let chars: Vec<char> = tokens.chars().collect();
    !key_value_starts(&chars, key).is_empty()
}

/// Every key written at depth 0 of `tokens`, in order — both the `key = value`
/// and the `key(...)` group forms.
///
/// Used to reject unknown leaves inside a style block. `validate_known_keys`
/// can't do that job: it walks `syn`'s meta tree, which only reaches an
/// attribute's top level, so a typo nested inside `line_style(...)` was
/// previously invisible. This scan is literal-aware like every other one here,
/// so a `key =` written *inside* a selector string is content, not a key.
fn top_level_keys(tokens: &str) -> Vec<String> {
    let chars: Vec<char> = tokens.chars().collect();
    let mut keys = Vec::new();
    let mut depth = 0usize;
    let mut i = 0usize;
    while i < chars.len() {
        if let Some(end) = string_literal_end(&chars, i) {
            i = end;
            continue;
        }
        match chars[i] {
            '(' => {
                depth += 1;
                i += 1;
            }
            ')' => {
                depth = depth.saturating_sub(1);
                i += 1;
            }
            c if depth == 0 && (c.is_alphabetic() || c == '_') => {
                let start = i;
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                let ident: String = chars[start..i].iter().collect();
                let mut j = i;
                while j < chars.len() && chars[j].is_whitespace() {
                    j += 1;
                }
                // Only an identifier followed by `=` or `(` is a key; a bare
                // word is a value (`negated = true`, an array item, …).
                if matches!(chars.get(j), Some('=') | Some('(')) {
                    keys.push(ident);
                }
            }
            _ => i += 1,
        }
    }
    keys
}

/// The bare (unquoted) value token at `start`: everything up to the next
/// separator or whitespace.
fn scalar_at(chars: &[char], start: usize) -> String {
    chars[start..]
        .iter()
        .take_while(|c| !(**c == ',' || **c == ')' || c.is_whitespace()))
        .collect()
}

/// Extract the value of a `key = "..."` pair from a token string.
///
/// Both literal shapes work: `"{x : Node | @:(x.color) = \"Red\"}"` and the raw
/// `r#"{x : Node | @:(x.color) = "Red"}"#`. simple-graph-query 3.0 wants string
/// comparands quoted, so selectors carry quotes either escaped or raw.
///
/// The whole literal's source text goes to `syn`, which decodes it exactly as
/// rustc would. A `key` whose value is not a string literal (a const, a path)
/// is skipped in favour of a later occurrence, if any.
fn extract_string_from_tokens(tokens: &str, key: &str) -> Option<String> {
    let chars: Vec<char> = tokens.chars().collect();
    key_value_starts(&chars, key).into_iter().find_map(|start| {
        let end = string_literal_end(&chars, start)?;
        let literal: String = chars[start..end].iter().collect();
        syn::parse_str::<syn::LitStr>(&literal)
            .ok()
            .map(|lit| lit.value())
    })
}

fn extract_number_from_tokens(tokens: &str, key: &str) -> Option<u32> {
    let chars: Vec<char> = tokens.chars().collect();
    key_value_starts(&chars, key)
        .into_iter()
        .find_map(|start| scalar_at(&chars, start).parse::<u32>().ok())
}

fn extract_bool_from_tokens(tokens: &str, key: &str) -> Option<bool> {
    let chars: Vec<char> = tokens.chars().collect();
    key_value_starts(&chars, key)
        .into_iter()
        .find_map(|start| scalar_at(&chars, start).parse::<bool>().ok())
}

fn extract_float_from_tokens(tokens: &str, key: &str) -> Option<f64> {
    let chars: Vec<char> = tokens.chars().collect();
    key_value_starts(&chars, key)
        .into_iter()
        .find_map(|start| scalar_at(&chars, start).parse::<f64>().ok())
}

/// Length of the string literal starting at `chars[i]`, as an end index — or
/// `None` if no literal starts there.
///
/// Handles both shapes a selector can be written in: `"escaped \"Red\""` and
/// the raw form `r#"quoted "Red" verbatim"#` (any number of `#`). Every scan
/// over attribute token text goes through this, so a literal's *body* never
/// steers a scan — the quotes, parens, and `key = value` text inside a selector
/// are content, not syntax.
///
/// The `r` case needs no identifier-boundary check: a raw string is a single
/// token, so `r#"` only ever appears glued together, while a bare `r` ident
/// followed by a string prints with a space between them.
fn string_literal_end(chars: &[char], i: usize) -> Option<usize> {
    match chars.get(i)? {
        '"' => {
            let mut k = i + 1;
            let mut escaped = false;
            while k < chars.len() {
                match chars[k] {
                    _ if escaped => escaped = false,
                    '\\' => escaped = true,
                    '"' => return Some(k + 1),
                    _ => {}
                }
                k += 1;
            }
            None // unterminated
        }
        'r' => {
            let mut k = i + 1;
            let mut hashes = 0usize;
            while chars.get(k) == Some(&'#') {
                hashes += 1;
                k += 1;
            }
            if chars.get(k) != Some(&'"') {
                return None;
            }
            k += 1;
            // Raw strings have no escapes: the body runs to the first `"`
            // followed by as many `#` as opened it.
            while k < chars.len() {
                if chars[k] == '"'
                    && chars[k + 1..]
                        .iter()
                        .take(hashes)
                        .filter(|c| **c == '#')
                        .count()
                        == hashes
                {
                    return Some(k + 1 + hashes);
                }
                k += 1;
            }
            None // unterminated
        }
        _ => None,
    }
}

/// Remove the contents of every parenthesized group from a token string,
/// leaving only the top-level `key = value` pairs (the group keys survive as
/// `key ()`). Used so flat-key extraction never matches a key *inside* a
/// style block — e.g. `weight` inside `line_style(weight = 2.0)` must not be
/// read as a top-level legacy `weight`. Skips string literals whole, so parens
/// inside selector strings don't unbalance the scan.
fn strip_groups(tokens: &str) -> String {
    let chars: Vec<char> = tokens.chars().collect();
    let mut out = String::with_capacity(tokens.len());
    let mut depth = 0usize;
    let mut i = 0usize;
    while i < chars.len() {
        if let Some(end) = string_literal_end(&chars, i) {
            if depth == 0 {
                out.extend(&chars[i..end]);
            }
            i = end;
            continue;
        }
        match chars[i] {
            '(' => {
                if depth == 0 {
                    out.push('(');
                }
                depth += 1;
            }
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    out.push(')');
                }
            }
            c => {
                if depth == 0 {
                    out.push(c);
                }
            }
        }
        i += 1;
    }
    out
}

/// Extract the inner token text of a top-level `key ( ... )` group from a
/// token string, or `None` if the key has no group at depth 0. Literal-aware
/// and balanced, so nested groups (`add_edge(points = ..., line_style(...))`)
/// and parens inside selector strings are handled.
fn extract_group_from_tokens(tokens: &str, key: &str) -> Option<String> {
    let chars: Vec<char> = tokens.chars().collect();
    let key_chars: Vec<char> = key.chars().collect();
    let mut depth = 0usize;
    let mut i = 0usize;
    while i < chars.len() {
        if let Some(end) = string_literal_end(&chars, i) {
            i = end;
            continue;
        }
        match chars[i] {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ => {
                // Match `key` at depth 0, on an identifier boundary.
                if depth == 0
                    && chars[i..].starts_with(&key_chars[..])
                    && (i == 0 || !(chars[i - 1].is_alphanumeric() || chars[i - 1] == '_'))
                {
                    let mut j = i + key_chars.len();
                    // Identifier must end exactly at the key.
                    if j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '_') {
                        i += 1;
                        continue;
                    }
                    while j < chars.len() && chars[j].is_whitespace() {
                        j += 1;
                    }
                    if j < chars.len() && chars[j] == '(' {
                        // Collect the balanced group body.
                        let mut body = String::new();
                        let mut inner_depth = 1usize;
                        let mut k = j + 1;
                        while k < chars.len() {
                            if let Some(end) = string_literal_end(&chars, k) {
                                body.extend(&chars[k..end]);
                                k = end;
                                continue;
                            }
                            match chars[k] {
                                '(' => {
                                    inner_depth += 1;
                                    body.push('(');
                                }
                                ')' => {
                                    inner_depth -= 1;
                                    if inner_depth == 0 {
                                        return Some(body);
                                    }
                                    body.push(')');
                                }
                                c => body.push(c),
                            }
                            k += 1;
                        }
                        return None; // unbalanced — treat as absent
                    }
                }
            }
        }
        i += 1;
    }
    None
}

/// Extract the items of a `key = [...]` array from a token string.
///
/// Quoted items are decoded as literals (so `]` or a comma inside one is
/// content, not a delimiter); bare items are taken as written, which is what
/// the flat scan used to do for every item.
fn extract_array_from_tokens(tokens: &str, key: &str) -> Option<Vec<String>> {
    let chars: Vec<char> = tokens.chars().collect();
    key_value_starts(&chars, key).into_iter().find_map(|start| {
        if chars.get(start) != Some(&'[') {
            return None;
        }
        let mut items = Vec::new();
        let mut bare = String::new();
        let mut i = start + 1;
        while i < chars.len() && chars[i] != ']' {
            if let Some(end) = string_literal_end(&chars, i) {
                let literal: String = chars[i..end].iter().collect();
                if let Ok(lit) = syn::parse_str::<syn::LitStr>(&literal) {
                    items.push(lit.value());
                }
                i = end;
                continue;
            }
            match chars[i] {
                ',' => {
                    if !bare.trim().is_empty() {
                        items.push(bare.trim().to_string());
                    }
                    bare.clear();
                }
                c => bare.push(c),
            }
            i += 1;
        }
        if i >= chars.len() {
            return None; // unterminated — treat as absent
        }
        if !bare.trim().is_empty() {
            items.push(bare.trim().to_string());
        }
        Some(items)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    fn dep_for(
        attr: Attribute,
        name: &str,
    ) -> Option<(&'static DeprecationSpec, Option<&'static str>)> {
        deprecation_for(&attr, name)
    }

    /// The field-based group left the language in spytial-core 5.1.0. It is
    /// no longer something the shim can warn about — the engine would refuse
    /// the spec — so the parser rejects it, and the error has to name the
    /// replacement rather than read as a mistyped key.
    #[test]
    fn the_removed_group_shape_is_an_error_naming_its_replacement() {
        let attr: Attribute = parse_quote!(#[group(field = "rel", group_on = 1, add_to_group = 0)]);
        let msg = parse_group_args(&attr)
            .expect_err("the field-based group no longer parses")
            .to_string();
        assert!(msg.contains("removed"), "must say it was removed:\n{msg}");
        assert!(
            msg.contains("selector = "),
            "must show the selector form to write instead:\n{msg}"
        );
        assert!(
            dep_for(attr, "group").is_none(),
            "no deprecation entry: the manifest no longer lists the shape at all",
        );

        assert!(
            dep_for(
                parse_quote!(#[group(selector = "Team.members", name = "Team")]),
                "group"
            )
            .is_none(),
            "the selector-based group is the current form and must stay quiet",
        );
    }

    /// `icon` and `atom_color` are deprecated outright — every spelling warns.
    #[test]
    fn wholly_deprecated_attributes_warn_on_any_spelling() {
        for attr in [
            parse_quote!(#[icon(path = "a.svg")]),
            parse_quote!(#[icon(selector = "P", path = "a.svg", show_labels = true)]),
        ] {
            let (dep, key) = dep_for(attr, "icon").expect("icon is deprecated");
            assert_eq!(
                key, None,
                "no key selects it; the attribute itself is deprecated"
            );
            assert_eq!(dep.replaced_by, "atom_style");
        }

        let (dep, _) = dep_for(
            parse_quote!(#[atom_color(selector = "N", value = "red")]),
            "atom_color",
        )
        .expect("atom_color is deprecated");
        assert_eq!(dep.replaced_by, "atom_style");
    }

    /// The legacy flat trio warns; the block form it was replaced by does not.
    #[test]
    fn legacy_edge_keys_warn_but_the_block_form_does_not() {
        for (attr, expected) in [
            (
                parse_quote!(#[edge_style(field = "n", value = "blue")]),
                "value",
            ),
            (
                parse_quote!(#[edge_style(field = "n", style = "dotted")]),
                "style",
            ),
            (
                parse_quote!(#[edge_style(field = "n", weight = 2.0)]),
                "weight",
            ),
        ] {
            let (_, key) = dep_for(attr, "edge_style").expect("legacy flat keys are deprecated");
            assert_eq!(key, Some(expected));
        }

        assert!(
            dep_for(
                parse_quote!(#[edge_style(field = "n", line_style(color = "blue"))]),
                "edge_style"
            )
            .is_none(),
            "the block form is current",
        );
    }

    /// A legacy key name appearing *inside* a style block is not a legacy key.
    /// Without the group-stripping this would warn on the current form.
    #[test]
    fn a_leaf_inside_a_block_is_not_a_top_level_legacy_key() {
        assert!(
            dep_for(
                parse_quote!(#[edge_style(field = "n", line_style(weight = 2.0, pattern = "dotted"))]),
                "edge_style"
            )
            .is_none(),
            "`weight` inside line_style(...) is the block's leaf, not the legacy flat key",
        );
    }

    /// The expansion has to be an actual `#[deprecated]` item carrying the
    /// manifest's note — that item is the only reason rustc says anything.
    #[test]
    fn the_shim_is_a_deprecated_item_carrying_the_note() {
        let attr: Attribute = parse_quote!(#[icon(path = "a.svg")]);
        let shim = deprecation_shim(&attr, "icon")
            .expect("icon is deprecated")
            .to_string();

        assert!(
            shim.contains("deprecated"),
            "not a deprecation shim:\n{shim}"
        );
        assert!(
            shim.contains("icon_is_deprecated"),
            "marker name is part of the diagnostic:\n{shim}"
        );
        assert!(
            shim.contains("atom_style"),
            "the note must name the replacement:\n{shim}"
        );
        assert!(
            shim.contains(spec_tables::SPYTIAL_CORE_VERSION),
            "the note must say which spytial-core deprecated it:\n{shim}",
        );
    }

    /// The `source` text has to be recognisable as the attribute the user
    /// wrote: conventional spacing, and every literal verbatim.
    #[test]
    fn source_text_renders_the_attribute_as_written() {
        let attr: Attribute = parse_quote!(
            #[orientation(selector = "{x, y : RBNode | x->y in left}", directions = ["left",   "below"], negated = true)]
        );
        assert_eq!(
            render_attribute(&attr),
            r#"#[orientation(selector = "{x, y : RBNode | x->y in left}", directions = ["left", "below"], negated = true)]"#
        );

        let attr: Attribute = parse_quote!(
            #[atom_style(selector = r#"{x : RBNode | @:(x.color) = "Red"}"#, border_style(color = "red", width = 2.0))]
        );
        assert_eq!(
            render_attribute(&attr),
            r##"#[atom_style(selector = r#"{x : RBNode | @:(x.color) = "Red"}"#, border_style(color = "red", width = 2.0))]"##
        );

        let attr: Attribute = parse_quote!(#[hide_atom(selector = "Color + u32 + None")]);
        assert_eq!(
            render_attribute(&attr),
            r#"#[hide_atom(selector = "Color + u32 + None")]"#
        );
    }

    /// The derive stamps a `source` on every rule but `flag`, which is a bare
    /// scalar. That has to stay in step with what spytial-core says it will
    /// accept: a form that stops supporting `source` must stop being stamped,
    /// and a form that starts supporting it should start.
    #[test]
    fn source_is_stamped_on_exactly_the_forms_that_accept_it() {
        for spec in spec_tables::ATTRS {
            let stamped = spec.attr != "flag";
            assert_eq!(
                spec_tables::SOURCE_SUPPORTED_BY.contains(&spec.yaml_key),
                stamped,
                "#[{}] (`{}`): the derive {} a `source` but the manifest says spytial-core {} one",
                spec.attr,
                spec.yaml_key,
                if stamped { "stamps" } else { "does not stamp" },
                if stamped {
                    "does not accept"
                } else {
                    "accepts"
                },
            );
        }
    }

    /// A `when_any_key` naming a key the attribute does not accept, or a
    /// `replaced_by` naming an attribute that does not exist, would produce a
    /// warning that can never fire or that points nowhere. Neither is visible
    /// at generation time, because both tables are generated independently.
    #[test]
    fn every_deprecation_refers_to_keys_and_attributes_that_exist() {
        for dep in spec_tables::DEPRECATIONS {
            let spec = spec_tables::attr_spec(dep.attr)
                .unwrap_or_else(|| panic!("`{}` is not an authoring attribute", dep.attr));
            for key in dep.when_any_key {
                assert!(
                    spec.keys.contains(key),
                    "#[{}] is said to be deprecated when `{key}` is present, but it does not \
                     accept that key — the warning could never fire",
                    dep.attr,
                );
            }
            assert!(
                spec_tables::attr_spec(dep.replaced_by).is_some(),
                "#[{}] points at `{}` as its replacement, which is not an authoring attribute",
                dep.attr,
                dep.replaced_by,
            );
        }
    }
}
