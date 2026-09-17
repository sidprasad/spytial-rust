//! Reconstructing Rust values from the relational [`jsondata`](crate::jsondata)
//! shape — the inverse of [`crate::export`].
//!
//! [`from_datum`] is a serde `Deserializer` over a
//! [`JsonDataInstance`](crate::jsondata::JsonDataInstance): it walks the flat
//! atom/relation graph from a root atom and rebuilds a genuine `T`, so `{:?}`
//! runs the real [`Debug`] impl. [`replit`] is
//! `format!("{:?}", from_datum::<T>(..)?)` — the REPL/`Debug`-equivalent string.
//!
//! # Scope
//!
//! Covers the full serde tree model: primitives (including `i128`/`u128`),
//! byte arrays, `Option`, sequences, tuples, tuple/newtype/unit structs, maps,
//! structs, and all enum variant shapes. That
//! is exactly what [`crate::export`] produces, which is acyclic by construction
//! — arena/index "graphs" round-trip as plain data (a self-loop is the integer
//! index `Some(0)`, not a pointer). True `Rc<RefCell>` pointer cycles are not
//! representable in the exported form and are out of scope; sharing/aliasing is
//! not preserved (export duplicates it), which is invisible to `==`/`{:?}`.
//!
//! Nested `Option` round-trips faithfully: `export` unwraps `Some(x)` (so the
//! common case stays a single clean atom) but inserts a `Some` wrapper atom when
//! the inner is a `None`/`Some`, so `Some(None)` is distinct from `None` and any
//! nesting depth is recoverable.
//!
//! # Self-describing reconstruction
//!
//! Reconstruction is type-driven: `T` says what to expect and this module goes
//! looking for it. Three serde representations cannot work that way, because
//! they buffer a value *before* they know its type — `#[serde(flatten)]`,
//! `#[serde(untagged)]`, and the internally and adjacently tagged enum forms.
//! They call [`serde::de::Deserializer::deserialize_any`], which answers from the atom's
//! own `type` and outgoing relations rather than from `T`. See that method for
//! the ambiguous shapes it cannot tell apart.
//!
//! What `Serialize` never emits stays unrecoverable, whatever reify does:
//! `#[serde(skip)]` hides a field from the datum, so `Deserialize` fills it
//! from `Default` and `{:?}` disagrees. That is a limit of `Serialize` as the
//! inspection mechanism, not of the relational form.

use crate::jsondata::{IAtom, ITuple, JsonDataInstance};
use serde::de::{
    self, DeserializeOwned, DeserializeSeed, Deserializer, EnumAccess, MapAccess, SeqAccess,
    VariantAccess, Visitor,
};
use std::collections::HashMap;
use std::fmt::{self, Debug, Display};

/// Error produced while reconstructing a value from a
/// [`JsonDataInstance`].
#[derive(Debug, Clone)]
pub struct ReifyError(String);

impl ReifyError {
    fn msg(s: impl Into<String>) -> Self {
        ReifyError(s.into())
    }

    /// Borrow the underlying error message.
    pub fn message(&self) -> &str {
        &self.0
    }
}

impl Display for ReifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "reify error: {}", self.0)
    }
}

impl std::error::Error for ReifyError {}

impl de::Error for ReifyError {
    fn custom<T: Display>(msg: T) -> Self {
        ReifyError(msg.to_string())
    }
}

/// Reconstruct a `T` from a data instance, starting at its root atom.
///
/// The root is the atom that no relation targets (for `export` output, the
/// top-level value). Use [`from_datum_root`] to start from a specific atom id.
pub fn from_datum<T: DeserializeOwned>(datum: &JsonDataInstance) -> Result<T, ReifyError> {
    from_datum_root(datum, find_root(datum)?)
}

/// The root atom: the first atom (in serialization order) that no relation
/// targets. Robust to the `Some`-wrapper case, where `export` emits the wrapper
/// *after* its inner — so `atoms[0]` is not always the root.
fn find_root(datum: &JsonDataInstance) -> Result<&str, ReifyError> {
    if datum.atoms.is_empty() {
        return Err(ReifyError::msg("empty data instance: no atoms"));
    }
    let ids: std::collections::HashSet<&str> = datum.atoms.iter().map(|a| a.id.as_str()).collect();
    let mut targeted: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for rel in &datum.relations {
        for t in &rel.tuples {
            // Position 0 is the source; every later position names an atom.
            for tgt in t.atoms.iter().skip(1) {
                if ids.contains(tgt.as_str()) {
                    targeted.insert(tgt.as_str());
                }
            }
        }
    }
    datum
        .atoms
        .iter()
        .map(|a| a.id.as_str())
        .find(|id| !targeted.contains(id))
        .ok_or_else(|| {
            ReifyError::msg("no root atom: every atom is referenced (unexpected for export output)")
        })
}

/// Reconstruct a `T` starting from an explicit root atom id.
pub fn from_datum_root<T: DeserializeOwned>(
    datum: &JsonDataInstance,
    root_id: &str,
) -> Result<T, ReifyError> {
    let index = Index::build(datum);
    let root_key = *index
        .atoms
        .get_key_value(root_id)
        .ok_or_else(|| ReifyError::msg(format!("root atom not found: {root_id}")))?
        .0;
    T::deserialize(NodeDeserializer {
        index: &index,
        atom_id: root_key,
    })
}

/// Reconstruct a `T` and return its `Debug` string — the REPL-equivalent output.
///
/// Equivalent to `format!("{:?}", from_datum::<T>(datum)?)`. Because the value
/// is parsed into a real `T` and Rust's own `Debug` re-renders it, this matches
/// `format!("{:?}", original)` exactly (custom `Debug` impls included).
pub fn replit<T: DeserializeOwned + Debug>(datum: &JsonDataInstance) -> Result<String, ReifyError> {
    Ok(format!("{:?}", from_datum::<T>(datum)?))
}

/// [`replit`] starting from an explicit root atom id.
pub fn replit_root<T: DeserializeOwned + Debug>(
    datum: &JsonDataInstance,
    root_id: &str,
) -> Result<String, ReifyError> {
    Ok(format!("{:?}", from_datum_root::<T>(datum, root_id)?))
}

// ---------------------------------------------------------------------------
// Internal index over the flat atom/relation graph
// ---------------------------------------------------------------------------

struct Index<'a> {
    atoms: HashMap<&'a str, &'a IAtom>,
    /// source atom id -> relation name -> tuples whose first atom is that source
    out: HashMap<&'a str, HashMap<&'a str, Vec<&'a ITuple>>>,
}

impl<'a> Index<'a> {
    fn build(d: &'a JsonDataInstance) -> Self {
        let atoms = d.atoms.iter().map(|a| (a.id.as_str(), a)).collect();
        let mut out: HashMap<&str, HashMap<&str, Vec<&ITuple>>> = HashMap::new();
        for rel in &d.relations {
            for t in &rel.tuples {
                if let Some(src) = t.atoms.first() {
                    out.entry(src.as_str())
                        .or_default()
                        .entry(rel.name.as_str())
                        .or_default()
                        .push(t);
                }
            }
        }
        Index { atoms, out }
    }

    fn atom(&self, id: &str) -> Result<&'a IAtom, ReifyError> {
        self.atoms
            .get(id)
            .copied()
            .ok_or_else(|| ReifyError::msg(format!("atom not found: {id}")))
    }

    /// The single target of a binary relation `rel` from `src` (its second atom).
    fn single_target(&self, src: &str, rel: &str) -> Result<&'a str, ReifyError> {
        self.out
            .get(src)
            .and_then(|m| m.get(rel))
            .and_then(|v| v.first())
            .and_then(|t| t.atoms.get(1))
            .map(|s| s.as_str())
            .ok_or_else(|| ReifyError::msg(format!("missing relation '{rel}' from atom {src}")))
    }

    /// Element atom ids for a sequence/tuple, ordered by the `idx` position.
    fn seq_elems(&self, src: &str) -> Vec<&'a str> {
        let mut items: Vec<(usize, &'a str)> = Vec::new();
        if let Some(tuples) = self.out.get(src).and_then(|m| m.get("idx")) {
            for t in tuples {
                if let (Some(pos), Some(elem)) = (t.atoms.get(1), t.atoms.get(2)) {
                    // Current export output stores the position as an ordinary
                    // value atom. Fall back to the raw tuple entry so data from
                    // older spytial versions remains reifiable.
                    let position_value = self
                        .atoms
                        .get(pos.as_str())
                        .map(|atom| atom.label.as_str())
                        .unwrap_or(pos.as_str());
                    if let Ok(p) = position_value.parse::<usize>() {
                        items.push((p, elem.as_str()));
                    }
                }
            }
        }
        items.sort_by_key(|(p, _)| *p);
        items.into_iter().map(|(_, e)| e).collect()
    }

    /// (key atom id, value atom id) pairs for a map's `map_entry` relation.
    fn map_entries(&self, src: &str) -> Vec<(&'a str, &'a str)> {
        let mut v = Vec::new();
        if let Some(tuples) = self.out.get(src).and_then(|m| m.get("map_entry")) {
            for t in tuples {
                if let (Some(k), Some(val)) = (t.atoms.get(1), t.atoms.get(2)) {
                    v.push((k.as_str(), val.as_str()));
                }
            }
        }
        v
    }

    /// (field name, value atom id) pairs for a struct's field relations.
    fn struct_fields(&self, src: &str) -> Vec<(&'a str, &'a str)> {
        let mut v = Vec::new();
        if let Some(m) = self.out.get(src) {
            for (name, tuples) in m {
                if let Some(t) = tuples.first() {
                    if let Some(target) = t.atoms.get(1) {
                        v.push((*name, target.as_str()));
                    }
                }
            }
        }
        v
    }
}

// ---------------------------------------------------------------------------
// Node deserializer: drives serde from a single atom
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct NodeDeserializer<'i, 'a> {
    index: &'i Index<'a>,
    atom_id: &'a str,
}

impl<'i, 'a> NodeDeserializer<'i, 'a> {
    fn atom(&self) -> Result<&'a IAtom, ReifyError> {
        self.index.atom(self.atom_id)
    }

    fn child(&self, id: &'a str) -> Self {
        NodeDeserializer {
            index: self.index,
            atom_id: id,
        }
    }

    fn parse<T>(&self) -> Result<T, ReifyError>
    where
        T: std::str::FromStr,
        T::Err: Display,
    {
        let a = self.atom()?;
        a.label.parse::<T>().map_err(|e| {
            ReifyError::msg(format!(
                "could not parse {} label '{}': {e}",
                a.r#type, a.label
            ))
        })
    }

    /// The user-named-atom half of [`Deserializer::deserialize_any`]: an atom
    /// whose `type` is a struct or enum name rather than one of export's
    /// built-ins.
    ///
    /// A struct becomes a map of its fields. An externally tagged enum variant
    /// becomes the single-entry map self-describing formats use for one,
    /// `{variant: payload}`, or the bare variant name when it carries nothing.
    fn deserialize_named_any<'de, V: Visitor<'de>>(
        self,
        visitor: V,
    ) -> Result<V::Value, ReifyError> {
        let a = self.atom()?;

        // Ask the label first. Export writes a struct's own type name as its
        // label and an externally tagged variant's *variant* name as its own,
        // so this is what separates `Shape::Rect { w }` from a plain struct —
        // both are just a bag of named fields. It has to come before any test
        // on relation names, because a struct is free to have a field called
        // `idx` or `variant_value`, which would otherwise read as an enum
        // variant's payload rather than as the field it is.
        if a.label == a.r#type {
            return self.deserialize_struct("", &[], visitor);
        }

        let label = a.label.as_str();
        let Some(relations) = self.index.out.get(self.atom_id) else {
            // No outgoing relations: usually a unit variant, which
            // self-describing formats write as the bare variant name. An
            // empty struct or tuple variant has the same exported shape and
            // cannot be reconstructed through deserialize_any.
            return visitor.visit_str(label);
        };

        // A tuple variant's `idx` tuples are ternary — (variant, position,
        // element). A struct variant with a field *named* `idx` emits binary
        // ones, so arity tells the two apart rather than the name alone.
        let is_tuple_variant = relations
            .get("idx")
            .is_some_and(|tuples| tuples.iter().any(|t| t.atoms.len() >= 3));
        if is_tuple_variant {
            let payload = VariantPayload::Tuple(self.index, self.atom_id);
            return visitor.visit_map(OneEntry::new(label, payload));
        }

        // A newtype variant carries `variant_value` and nothing else. A struct
        // variant with a single field of that exact name is written the same
        // way and is read as a newtype variant; that pathology is documented on
        // `deserialize_any` alongside `enum E { E }`. One further field is
        // enough to tell them apart, so the ambiguity is as narrow as it looks.
        if relations.len() == 1 && relations.contains_key("variant_value") {
            let inner = self.index.single_target(self.atom_id, "variant_value")?;
            let payload = VariantPayload::Newtype(self.child(inner));
            return visitor.visit_map(OneEntry::new(label, payload));
        }

        let payload = VariantPayload::Struct(self.index, self.atom_id);
        visitor.visit_map(OneEntry::new(label, payload))
    }
}

macro_rules! deserialize_parsed {
    ($method:ident, $visit:ident, $t:ty) => {
        fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, ReifyError> {
            visitor.$visit(self.parse::<$t>()?)
        }
    };
}

impl<'i, 'a, 'de> Deserializer<'de> for NodeDeserializer<'i, 'a> {
    type Error = ReifyError;

    /// Reconstruct without being told what to expect, by reading the atom's own
    /// `type` and outgoing relations.
    ///
    /// Reify is type-driven everywhere else, and this method exists for the
    /// three serde representations that buffer a value before they know its
    /// type: `#[serde(flatten)]`, `#[serde(untagged)]`, and internally or
    /// adjacently tagged enums. All three ask the format to describe itself.
    ///
    /// Built-in atom types answer directly — `"map"`, `"sequence"`, `"i32"` and
    /// the rest say what they are. A user-named atom is a struct or an enum
    /// variant, and those are told apart by `label`: export writes the type's
    /// own name as the label of a struct, and the *variant's* name as the label
    /// of an externally tagged variant.
    ///
    /// Several shapes defeat that, all of them pathological and all of them
    /// documented rather than guessed at:
    ///
    /// * `enum E { E }` — a variant named after its own enum, which reads back
    ///   as a struct.
    /// * A struct named after one of export's built-in atom types (`map`,
    ///   `sequence`, `unit`, ...), which is answered as that built-in.
    /// * `enum E { V { variant_value: T } }` — an externally tagged struct
    ///   variant whose only field is named `variant_value`, which is written
    ///   exactly like the newtype variant `E::V(T)` and reads back as one. A
    ///   second field is enough to separate them.
    /// * A zero-field struct variant or zero-element tuple variant — it has
    ///   no outgoing relation, just like a unit variant, and reads back as a
    ///   bare variant name in this self-describing path.
    ///
    /// A struct field named `idx` is binary where a
    /// tuple variant's `idx` is ternary, and a field named `map_entry` or
    /// `value` only ever competes with a built-in atom type, never with a
    /// user-named one.
    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, ReifyError> {
        let a = self.atom()?;
        match a.r#type.as_str() {
            "bool" => visitor.visit_bool(self.parse()?),
            "i8" | "i16" | "i32" | "i64" => visitor.visit_i64(self.parse()?),
            "i128" => visitor.visit_i128(self.parse()?),
            "u8" | "u16" | "u32" | "u64" => visitor.visit_u64(self.parse()?),
            "u128" => visitor.visit_u128(self.parse()?),
            "f32" | "f64" => visitor.visit_f64(self.parse()?),
            "char" => self.deserialize_char(visitor),
            "string" => visitor.visit_str(a.label.as_str()),
            // Without this arm a byte array falls through to the user-named
            // branch, where its `[1, 2, 3]` label is handed over as a variant
            // name and the byte-buffer visitor never sees any bytes.
            "bytes" => self.deserialize_bytes(visitor),
            "unit" | "unit_struct" => visitor.visit_unit(),
            "None" => visitor.visit_none(),
            "Some" => {
                let inner = self.index.single_target(self.atom_id, "value")?;
                visitor.visit_some(self.child(inner))
            }
            // A newtype struct is transparent in every self-describing format.
            "newtype_struct" => {
                let inner = self.index.single_target(self.atom_id, "value")?;
                self.child(inner).deserialize_any(visitor)
            }
            "sequence" | "tuple" | "tuple_struct" => self.deserialize_seq(visitor),
            "map" => self.deserialize_map(visitor),
            _ => self.deserialize_named_any(visitor),
        }
    }

    deserialize_parsed!(deserialize_bool, visit_bool, bool);
    deserialize_parsed!(deserialize_i8, visit_i8, i8);
    deserialize_parsed!(deserialize_i16, visit_i16, i16);
    deserialize_parsed!(deserialize_i32, visit_i32, i32);
    deserialize_parsed!(deserialize_i64, visit_i64, i64);
    deserialize_parsed!(deserialize_i128, visit_i128, i128);
    deserialize_parsed!(deserialize_u8, visit_u8, u8);
    deserialize_parsed!(deserialize_u16, visit_u16, u16);
    deserialize_parsed!(deserialize_u32, visit_u32, u32);
    deserialize_parsed!(deserialize_u64, visit_u64, u64);
    deserialize_parsed!(deserialize_u128, visit_u128, u128);
    deserialize_parsed!(deserialize_f32, visit_f32, f32);
    deserialize_parsed!(deserialize_f64, visit_f64, f64);

    fn deserialize_char<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, ReifyError> {
        let a = self.atom()?;
        let c = a
            .label
            .chars()
            .next()
            .ok_or_else(|| ReifyError::msg("empty char label"))?;
        visitor.visit_char(c)
    }

    fn deserialize_str<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, ReifyError> {
        let a = self.atom()?;
        visitor.visit_str(a.label.as_str())
    }

    fn deserialize_string<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, ReifyError> {
        let a = self.atom()?;
        visitor.visit_string(a.label.clone())
    }

    fn deserialize_bytes<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, ReifyError> {
        let a = self.atom()?;
        visitor.visit_byte_buf(parse_bytes_label(&a.label)?)
    }

    fn deserialize_byte_buf<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, ReifyError> {
        self.deserialize_bytes(visitor)
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, ReifyError> {
        let a = self.atom()?;
        match a.r#type.as_str() {
            // The `None` value atom.
            "None" => visitor.visit_none(),
            // `Some` wrapper — export inserts it only to keep an inner
            // `None`/`Some` distinct; descend through its `value` relation.
            "Some" => {
                let inner = self.index.single_target(self.atom_id, "value")?;
                visitor.visit_some(self.child(inner))
            }
            // Unwrapped `Some(x)`: the atom *is* `x`.
            _ => visitor.visit_some(self),
        }
    }

    fn deserialize_unit<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, ReifyError> {
        visitor.visit_unit()
    }

    fn deserialize_unit_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, ReifyError> {
        visitor.visit_unit()
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, ReifyError> {
        let inner = self.index.single_target(self.atom_id, "value")?;
        visitor.visit_newtype_struct(self.child(inner))
    }

    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, ReifyError> {
        let elems = self.index.seq_elems(self.atom_id);
        visitor.visit_seq(SeqWalker {
            index: self.index,
            elems,
            pos: 0,
        })
    }

    fn deserialize_tuple<V: Visitor<'de>>(
        self,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, ReifyError> {
        self.deserialize_seq(visitor)
    }

    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, ReifyError> {
        self.deserialize_seq(visitor)
    }

    fn deserialize_map<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, ReifyError> {
        let entries = self.index.map_entries(self.atom_id);
        visitor.visit_map(MapWalker {
            index: self.index,
            entries,
            pos: 0,
        })
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, ReifyError> {
        let fields = self.index.struct_fields(self.atom_id);
        visitor.visit_map(StructWalker {
            index: self.index,
            fields,
            pos: 0,
        })
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, ReifyError> {
        let a = self.atom()?;
        visitor.visit_enum(EnumWalker {
            index: self.index,
            atom_id: self.atom_id,
            variant: a.label.as_str(),
        })
    }

    fn deserialize_identifier<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, ReifyError> {
        self.deserialize_str(visitor)
    }

    fn deserialize_ignored_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, ReifyError> {
        visitor.visit_unit()
    }
}

// ---------------------------------------------------------------------------
// Access helpers for compound shapes
// ---------------------------------------------------------------------------

struct SeqWalker<'i, 'a> {
    index: &'i Index<'a>,
    elems: Vec<&'a str>,
    pos: usize,
}

impl<'i, 'a, 'de> SeqAccess<'de> for SeqWalker<'i, 'a> {
    type Error = ReifyError;

    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, ReifyError> {
        if self.pos >= self.elems.len() {
            return Ok(None);
        }
        let id = self.elems[self.pos];
        self.pos += 1;
        seed.deserialize(NodeDeserializer {
            index: self.index,
            atom_id: id,
        })
        .map(Some)
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.elems.len() - self.pos)
    }
}

struct MapWalker<'i, 'a> {
    index: &'i Index<'a>,
    entries: Vec<(&'a str, &'a str)>,
    pos: usize,
}

impl<'i, 'a, 'de> MapAccess<'de> for MapWalker<'i, 'a> {
    type Error = ReifyError;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, ReifyError> {
        if self.pos >= self.entries.len() {
            return Ok(None);
        }
        let (k, _) = self.entries[self.pos];
        seed.deserialize(NodeDeserializer {
            index: self.index,
            atom_id: k,
        })
        .map(Some)
    }

    fn next_value_seed<Vv: DeserializeSeed<'de>>(
        &mut self,
        seed: Vv,
    ) -> Result<Vv::Value, ReifyError> {
        let (_, v) = self.entries[self.pos];
        self.pos += 1;
        seed.deserialize(NodeDeserializer {
            index: self.index,
            atom_id: v,
        })
    }
}

struct StructWalker<'i, 'a> {
    index: &'i Index<'a>,
    fields: Vec<(&'a str, &'a str)>,
    pos: usize,
}

impl<'i, 'a, 'de> MapAccess<'de> for StructWalker<'i, 'a> {
    type Error = ReifyError;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, ReifyError> {
        if self.pos >= self.fields.len() {
            return Ok(None);
        }
        let (name, _) = self.fields[self.pos];
        seed.deserialize(IdentDeserializer(name)).map(Some)
    }

    fn next_value_seed<Vv: DeserializeSeed<'de>>(
        &mut self,
        seed: Vv,
    ) -> Result<Vv::Value, ReifyError> {
        let (_, target) = self.fields[self.pos];
        self.pos += 1;
        seed.deserialize(NodeDeserializer {
            index: self.index,
            atom_id: target,
        })
    }
}

struct EnumWalker<'i, 'a> {
    index: &'i Index<'a>,
    atom_id: &'a str,
    variant: &'a str,
}

impl<'i, 'a, 'de> EnumAccess<'de> for EnumWalker<'i, 'a> {
    type Error = ReifyError;
    type Variant = VariantWalker<'i, 'a>;

    fn variant_seed<V: DeserializeSeed<'de>>(
        self,
        seed: V,
    ) -> Result<(V::Value, Self::Variant), ReifyError> {
        let value = seed.deserialize(IdentDeserializer(self.variant))?;
        Ok((
            value,
            VariantWalker {
                index: self.index,
                atom_id: self.atom_id,
            },
        ))
    }
}

struct VariantWalker<'i, 'a> {
    index: &'i Index<'a>,
    atom_id: &'a str,
}

impl<'i, 'a, 'de> VariantAccess<'de> for VariantWalker<'i, 'a> {
    type Error = ReifyError;

    fn unit_variant(self) -> Result<(), ReifyError> {
        Ok(())
    }

    fn newtype_variant_seed<T: DeserializeSeed<'de>>(
        self,
        seed: T,
    ) -> Result<T::Value, ReifyError> {
        let inner = self.index.single_target(self.atom_id, "variant_value")?;
        seed.deserialize(NodeDeserializer {
            index: self.index,
            atom_id: inner,
        })
    }

    fn tuple_variant<V: Visitor<'de>>(
        self,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, ReifyError> {
        let elems = self.index.seq_elems(self.atom_id);
        visitor.visit_seq(SeqWalker {
            index: self.index,
            elems,
            pos: 0,
        })
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, ReifyError> {
        let fields = self.index.struct_fields(self.atom_id);
        visitor.visit_map(StructWalker {
            index: self.index,
            fields,
            pos: 0,
        })
    }
}

/// Parse the label `export` writes for a byte string: Rust's `{:?}` for a
/// `&[u8]`, i.e. `[1, 2, 3]` or `[]`.
fn parse_bytes_label(label: &str) -> Result<Vec<u8>, ReifyError> {
    let inner = label
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .ok_or_else(|| ReifyError::msg(format!("malformed bytes label '{label}'")))?;
    if inner.trim().is_empty() {
        return Ok(Vec::new());
    }
    inner
        .split(',')
        .map(|b| {
            b.trim()
                .parse::<u8>()
                .map_err(|e| ReifyError::msg(format!("bad byte in label '{label}': {e}")))
        })
        .collect()
}

/// Deserializer that yields a fixed string — used for struct field names and
/// enum variant names, which are relation names / atom labels, not atoms.
struct IdentDeserializer<'a>(&'a str);

impl<'a, 'de> Deserializer<'de> for IdentDeserializer<'a> {
    type Error = ReifyError;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, ReifyError> {
        visitor.visit_str(self.0)
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf option unit unit_struct newtype_struct seq tuple
        tuple_struct map struct enum identifier ignored_any
    }
}

/// The payload side of the single-entry map an externally tagged enum variant
/// takes in a self-describing format. Reached only from
/// [`NodeDeserializer::deserialize_named_any`], where the variant's shape is
/// already known, so `deserialize_any` is the only method that has to do work.
#[derive(Clone, Copy)]
enum VariantPayload<'i, 'a> {
    /// `Variant(x)` — the payload is the single atom `x`.
    Newtype(NodeDeserializer<'i, 'a>),
    /// `Variant(a, b)` — the payload is the variant atom's `idx` elements.
    Tuple(&'i Index<'a>, &'a str),
    /// `Variant { a, b }` — the payload is the variant atom's field relations.
    Struct(&'i Index<'a>, &'a str),
}

impl<'i, 'a, 'de> Deserializer<'de> for VariantPayload<'i, 'a> {
    type Error = ReifyError;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, ReifyError> {
        match self {
            VariantPayload::Newtype(node) => node.deserialize_any(visitor),
            VariantPayload::Tuple(index, atom_id) => visitor.visit_seq(SeqWalker {
                index,
                elems: index.seq_elems(atom_id),
                pos: 0,
            }),
            VariantPayload::Struct(index, atom_id) => visitor.visit_map(StructWalker {
                index,
                fields: index.struct_fields(atom_id),
                pos: 0,
            }),
        }
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf option unit unit_struct newtype_struct seq tuple
        tuple_struct map struct enum identifier ignored_any
    }
}

/// A map of exactly one entry, keyed by a fixed name: the shape a
/// self-describing format gives an externally tagged enum variant.
struct OneEntry<'i, 'a> {
    key: Option<&'a str>,
    value: VariantPayload<'i, 'a>,
}

impl<'i, 'a> OneEntry<'i, 'a> {
    fn new(key: &'a str, value: VariantPayload<'i, 'a>) -> Self {
        OneEntry {
            key: Some(key),
            value,
        }
    }
}

impl<'i, 'a, 'de> MapAccess<'de> for OneEntry<'i, 'a> {
    type Error = ReifyError;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, ReifyError> {
        match self.key.take() {
            Some(k) => seed.deserialize(IdentDeserializer(k)).map(Some),
            None => Ok(None),
        }
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(
        &mut self,
        seed: V,
    ) -> Result<V::Value, ReifyError> {
        seed.deserialize(self.value)
    }

    fn size_hint(&self) -> Option<usize> {
        Some(usize::from(self.key.is_some()))
    }
}
