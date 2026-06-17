//! Serialize Rust values into the atom/relation form the visualizer consumes
//! ([`JsonDataInstance`](crate::jsondata::JsonDataInstance)).
//!
//! This is a custom `serde` serializer; unlike a JSON encoder it keeps the
//! structural distinctions serde's data model exposes and JSON collapses: a
//! struct field, a map key, and a sequence index are different kinds of edge,
//! and each should survive into the diagram. Each value becomes an atom; its
//! parts become relations on that atom:
//!
//! | Rust type                  | Relation                            |
//! |----------------------------|-------------------------------------|
//! | `struct S { f: T }`        | `f(s, value)`, one per field        |
//! | `Vec<T>`, `[T; N]`, tuples | `idx(container, position, element)` |
//! | `HashMap<K, V>`            | `map_entry(map, key, value)`        |
//!
//! Field names become relation names because a field names a role fixed at
//! compile time; a map key stays an atom inside the tuple because it's runtime
//! data. Enum variants follow the same rules, keyed on the variant's atom.
//! Payload-free values — `None`, `()`, `true`/`false`, unit variants — are
//! interned as singletons so equal values share a node.

use crate::jsondata::*;
use crate::spytial_annotations::SpytialDecorators;
use serde::ser;
use serde::ser::{
    Serialize, SerializeMap, SerializeSeq, SerializeStruct, SerializeStructVariant, SerializeTuple,
    SerializeTupleStruct, SerializeTupleVariant, Serializer,
};
use std::collections::HashMap;
use std::fmt;

/// Export a Rust data structure to our JSON instance format using custom Serde serialization.
///
/// Returns an empty [`JsonDataInstance`] if the value's `Serialize` impl fails. Use
/// [`try_export_json_instance`] when you need to distinguish success from failure.
pub fn export_json_instance<T: Serialize>(value: &T) -> JsonDataInstance {
    try_export_json_instance(value).unwrap_or_else(|err| {
        eprintln!(
            "spytial: serialization failed, returning empty instance: {}",
            err.message()
        );
        JsonDataInstance {
            atoms: Vec::new(),
            relations: Vec::new(),
        }
    })
}

/// Fallible variant of [`export_json_instance`]: surfaces `Serialize` errors instead of
/// degrading to an empty instance.
pub fn try_export_json_instance<T: Serialize>(
    value: &T,
) -> Result<JsonDataInstance, SerializationError> {
    let mut serializer = JsonDataSerializer::new();
    value.serialize(&mut serializer)?;
    Ok(JsonDataInstance {
        atoms: serializer.atoms,
        relations: serializer.relations.into_values().collect(),
    })
}

/// Export a Rust data structure and collect SpyTial decorators from all encountered types.
/// Excludes the root type from collection to avoid double-counting.
///
/// Returns an empty instance and empty decorators on serialization failure.
pub fn export_json_instance_with_decorators<T: Serialize>(
    value: &T,
    root_type_name: &str,
) -> (JsonDataInstance, SpytialDecorators) {
    try_export_json_instance_with_decorators(value, root_type_name).unwrap_or_else(|err| {
        eprintln!(
            "spytial: serialization failed, returning empty instance: {}",
            err.message()
        );
        (
            JsonDataInstance {
                atoms: Vec::new(),
                relations: Vec::new(),
            },
            SpytialDecorators::default(),
        )
    })
}

/// Fallible variant of [`export_json_instance_with_decorators`].
pub fn try_export_json_instance_with_decorators<T: Serialize>(
    value: &T,
    root_type_name: &str,
) -> Result<(JsonDataInstance, SpytialDecorators), SerializationError> {
    let mut serializer = JsonDataSerializer::new();
    serializer.exclude_type = Some(root_type_name.to_string());
    value.serialize(&mut serializer)?;
    let instance = JsonDataInstance {
        atoms: serializer.atoms,
        relations: serializer.relations.into_values().collect(),
    };
    Ok((instance, serializer.collected_decorators))
}

/// Custom Serde serializer that preserves semantic structure for different collection types.
///
/// This type is an implementation detail of [`export_json_instance`] and
/// [`try_export_json_instance`] and is not part of the public API.
pub(crate) struct JsonDataSerializer {
    counter: usize,
    atoms: Vec<IAtom>,
    relations: HashMap<String, IRelation>,
    collected_decorators: SpytialDecorators,
    visited_types: std::collections::HashSet<String>,
    exclude_type: Option<String>,
    /// Cache for singleton atoms (like None, unit, etc.) that should be reused
    singleton_atoms: HashMap<(String, String), String>, // (type, label) -> atom_id
}

impl JsonDataSerializer {
    fn new() -> Self {
        Self {
            counter: 0,
            atoms: vec![],
            relations: HashMap::new(),
            collected_decorators: SpytialDecorators::default(),
            visited_types: std::collections::HashSet::new(),
            exclude_type: None,
            singleton_atoms: HashMap::new(),
        }
    }

    fn fresh_id(&mut self) -> String {
        let id = format!("atom{}", self.counter);
        self.counter += 1;
        id
    }

    fn emit_atom(&mut self, typ: &str, label: &str) -> String {
        let id = self.fresh_id();
        self.atoms.push(IAtom {
            id: id.clone(),
            r#type: typ.to_string(),
            label: label.to_string(),
        });
        id
    }

    /// Get or create a singleton atom - atoms that should only exist once
    /// (like None, unit, true, false, etc.)
    fn get_or_create_singleton(&mut self, typ: &str, label: &str) -> String {
        let key = (typ.to_string(), label.to_string());

        if let Some(existing_id) = self.singleton_atoms.get(&key) {
            return existing_id.clone();
        }

        let id = self.emit_atom(typ, label);
        self.singleton_atoms.insert(key, id.clone());
        id
    }

    fn push_relation(&mut self, name: &str, atoms: Vec<String>, types: Vec<&str>) {
        let types: Vec<String> = types.iter().map(|s| s.to_string()).collect();
        let tuple = ITuple {
            atoms,
            types: types.clone(),
        };

        let rel = self.relations.entry(name.to_string()).or_insert(IRelation {
            id: name.to_string(),
            name: name.to_string(),
            types,
            tuples: vec![],
        });
        rel.tuples.push(tuple);
    }

    /// Merge decorators for `type_name` into the collected set, if it has any
    /// registered and we haven't already visited it this run.
    ///
    /// Types register themselves at first call to `T::decorators()`, which the
    /// derive macro emits as part of the compile-time decorator walk. So by the
    /// time a value's `Serialize` impl visits a struct, the registry should
    /// already contain that struct's entry (when one exists).
    fn collect_decorators_for_type(&mut self, type_name: &str) {
        if let Some(ref exclude) = self.exclude_type {
            if type_name == exclude {
                return;
            }
        }

        if !self.visited_types.insert(type_name.to_string()) {
            return;
        }

        if let Some(type_decorators) = crate::spytial_annotations::get_type_decorators(type_name) {
            self.collected_decorators
                .constraints
                .extend(type_decorators.constraints);
            self.collected_decorators
                .directives
                .extend(type_decorators.directives);
        }
    }
}

/// Error returned by [`try_export_json_instance`] and friends when a value's
/// `Serialize` implementation fails. Wraps the underlying serializer message.
#[derive(Debug, Clone)]
pub struct SerializationError(String);

impl SerializationError {
    /// Borrow the underlying message produced by the failing `Serialize` impl.
    ///
    /// Useful when callers want to match on or transform the message
    /// programmatically rather than just printing the [`Display`](fmt::Display)
    /// form. The message does not include the `"Serialization error: "` prefix
    /// that `Display` adds.
    pub fn message(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for SerializationError {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SerializationError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Serialization error: {}", self.0)
    }
}

impl std::error::Error for SerializationError {}

impl ser::Error for SerializationError {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        SerializationError(msg.to_string())
    }
}

impl<'a> Serializer for &'a mut JsonDataSerializer {
    type Ok = String; // Return the atom ID
    type Error = SerializationError;

    // Collection serializers
    type SerializeSeq = SequenceSerializer<'a>;
    type SerializeTuple = TupleSerializer<'a>;
    type SerializeTupleStruct = TupleStructSerializer<'a>;
    type SerializeTupleVariant = TupleVariantSerializer<'a>;
    type SerializeMap = MapSerializer<'a>;
    type SerializeStruct = StructSerializer<'a>;
    type SerializeStructVariant = StructVariantSerializer<'a>;

    // Primitive types
    fn serialize_bool(self, v: bool) -> Result<Self::Ok, Self::Error> {
        // Booleans are singletons - there's only one true and one false
        Ok(self.get_or_create_singleton("bool", &v.to_string()))
    }

    fn serialize_i8(self, v: i8) -> Result<Self::Ok, Self::Error> {
        Ok(self.emit_atom("i8", &v.to_string()))
    }

    fn serialize_i16(self, v: i16) -> Result<Self::Ok, Self::Error> {
        Ok(self.emit_atom("i16", &v.to_string()))
    }

    fn serialize_i32(self, v: i32) -> Result<Self::Ok, Self::Error> {
        Ok(self.emit_atom("i32", &v.to_string()))
    }

    fn serialize_i64(self, v: i64) -> Result<Self::Ok, Self::Error> {
        Ok(self.emit_atom("i64", &v.to_string()))
    }

    fn serialize_u8(self, v: u8) -> Result<Self::Ok, Self::Error> {
        Ok(self.emit_atom("u8", &v.to_string()))
    }

    fn serialize_u16(self, v: u16) -> Result<Self::Ok, Self::Error> {
        Ok(self.emit_atom("u16", &v.to_string()))
    }

    fn serialize_u32(self, v: u32) -> Result<Self::Ok, Self::Error> {
        Ok(self.emit_atom("u32", &v.to_string()))
    }

    fn serialize_u64(self, v: u64) -> Result<Self::Ok, Self::Error> {
        Ok(self.emit_atom("u64", &v.to_string()))
    }

    fn serialize_f32(self, v: f32) -> Result<Self::Ok, Self::Error> {
        Ok(self.emit_atom("f32", &v.to_string()))
    }

    fn serialize_f64(self, v: f64) -> Result<Self::Ok, Self::Error> {
        Ok(self.emit_atom("f64", &v.to_string()))
    }

    fn serialize_char(self, v: char) -> Result<Self::Ok, Self::Error> {
        Ok(self.emit_atom("char", &v.to_string()))
    }

    fn serialize_str(self, v: &str) -> Result<Self::Ok, Self::Error> {
        Ok(self.emit_atom("string", v))
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<Self::Ok, Self::Error> {
        Ok(self.emit_atom("bytes", &format!("{:?}", v)))
    }

    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        Ok(self.get_or_create_singleton("None", "None"))
    }

    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<Self::Ok, Self::Error> {
        // Unwrap `Some` by default: `Some(x)` shares `x`'s atom, keeping the
        // diagram clean (`Option<T>` points straight at the `T`). But when the
        // inner is itself absent/optional — a `None` singleton or another `Some`
        // wrapper — insert a `Some` wrapper atom so `Some(None)` stays distinct
        // from `None` and arbitrarily nested options remain recoverable.
        let inner_id = value.serialize(&mut *self)?;
        let inner_is_optionish = self
            .atoms
            .iter()
            .find(|a| a.id == inner_id)
            .map(|a| a.r#type == "None" || a.r#type == "Some")
            .unwrap_or(false);
        if inner_is_optionish {
            let some_id = self.emit_atom("Some", "Some");
            self.push_relation(
                "value",
                vec![some_id.clone(), inner_id],
                vec!["Some", "atom"],
            );
            Ok(some_id)
        } else {
            Ok(inner_id)
        }
    }

    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        // Unit () is a singleton
        Ok(self.get_or_create_singleton("unit", "()"))
    }

    fn serialize_unit_struct(self, name: &str) -> Result<Self::Ok, Self::Error> {
        // Unit structs are singletons - only one instance of each unit struct type exists
        Ok(self.get_or_create_singleton("unit_struct", name))
    }

    fn serialize_unit_variant(
        self,
        enum_name: &str,
        _variant_index: u32,
        variant: &str,
    ) -> Result<Self::Ok, Self::Error> {
        // Unit variants are singletons: Color::Red is always the same value.
        Ok(self.get_or_create_singleton(enum_name, variant))
    }

    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        name: &str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error> {
        let struct_id = self.emit_atom("newtype_struct", name);
        let inner_id = value.serialize(&mut *self)?;
        self.push_relation(
            "value",
            vec![struct_id.clone(), inner_id],
            vec!["newtype_struct", "atom"],
        );
        Ok(struct_id)
    }

    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        enum_name: &str,
        _variant_index: u32,
        variant: &str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error> {
        let variant_id = self.emit_atom(enum_name, variant);
        let inner_id = value.serialize(&mut *self)?;
        self.push_relation(
            "variant_value",
            vec![variant_id.clone(), inner_id],
            vec![enum_name, "atom"],
        );
        Ok(variant_id)
    }

    // Vec/array/slice → idx(container, position, element).
    fn serialize_seq(self, len: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        let seq_id = self.emit_atom("sequence", &format!("seq[{}]", len.unwrap_or(0)));
        Ok(SequenceSerializer {
            serializer: self,
            seq_id,
            index: 0,
        })
    }

    // Tuple → idx; position is semantic (e.g. 0 = x, 1 = y).
    fn serialize_tuple(self, len: usize) -> Result<Self::SerializeTuple, Self::Error> {
        let tuple_id = self.emit_atom("tuple", &format!("tuple[{}]", len));
        Ok(TupleSerializer {
            serializer: self,
            tuple_id,
            index: 0,
        })
    }

    // Tuple struct → idx, like a tuple but typed by the struct name.
    fn serialize_tuple_struct(
        self,
        name: &str,
        _len: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        let struct_id = self.emit_atom("tuple_struct", name);
        Ok(TupleStructSerializer {
            serializer: self,
            struct_id,
            index: 0,
        })
    }

    // Tuple-like enum variant → idx, keyed on the variant atom.
    fn serialize_tuple_variant(
        self,
        enum_name: &str,
        _variant_index: u32,
        variant: &str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        let variant_id = self.emit_atom(enum_name, variant);
        Ok(TupleVariantSerializer {
            serializer: self,
            variant_id,
            variant_type: enum_name.to_string(),
            index: 0,
        })
    }

    // Map → map_entry(map, key, value). Keys stay atoms inside the tuple rather
    // than relation names: a key is runtime data, a struct field is a compile-time role.
    fn serialize_map(self, len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        let map_id = self.emit_atom("map", &format!("map[{}]", len.unwrap_or(0)));
        Ok(MapSerializer {
            serializer: self,
            map_id,
            key_id: None,
        })
    }

    // Struct → one relation per field, named after the field; the atom's type is
    // the struct name rather than a generic "struct".
    fn serialize_struct(
        self,
        name: &str,
        _len: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        let struct_id = self.emit_atom(name, name);
        self.collect_decorators_for_type(name);

        Ok(StructSerializer {
            serializer: self,
            struct_id,
            struct_type: name.to_string(),
        })
    }

    // Struct-like enum variant → field-name relations, like a struct, keyed on
    // the variant atom.
    fn serialize_struct_variant(
        self,
        enum_name: &str,
        _variant_index: u32,
        variant: &str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        let variant_id = self.emit_atom(enum_name, variant);
        Ok(StructVariantSerializer {
            serializer: self,
            variant_id,
            variant_type: enum_name.to_string(),
        })
    }
}

/// Emits `idx(seq, position, element)` for `Vec`/array/slice elements.
pub(crate) struct SequenceSerializer<'a> {
    serializer: &'a mut JsonDataSerializer,
    seq_id: String,
    index: usize,
}

impl<'a> SerializeSeq for SequenceSerializer<'a> {
    type Ok = String;
    type Error = SerializationError;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        let element_id = value.serialize(&mut *self.serializer)?;
        // idx(container, position, element) for O(1) indexable sequences
        self.serializer.push_relation(
            "idx",
            vec![self.seq_id.clone(), self.index.to_string(), element_id],
            vec!["sequence", "index", "atom"],
        );
        self.index += 1;
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(self.seq_id)
    }
}

// Tuples - heterogeneous, fixed positions
pub(crate) struct TupleSerializer<'a> {
    serializer: &'a mut JsonDataSerializer,
    tuple_id: String,
    index: usize,
}

impl<'a> SerializeTuple for TupleSerializer<'a> {
    type Ok = String;
    type Error = SerializationError;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        let element_id = value.serialize(&mut *self.serializer)?;
        // Tuples also use idx - fixed positional semantics
        self.serializer.push_relation(
            "idx",
            vec![self.tuple_id.clone(), self.index.to_string(), element_id],
            vec!["tuple", "index", "atom"],
        );
        self.index += 1;
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(self.tuple_id)
    }
}

// Tuple structs - named but positional
pub(crate) struct TupleStructSerializer<'a> {
    serializer: &'a mut JsonDataSerializer,
    struct_id: String,
    index: usize,
}

impl<'a> SerializeTupleStruct for TupleStructSerializer<'a> {
    type Ok = String;
    type Error = SerializationError;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        let field_id = value.serialize(&mut *self.serializer)?;
        // Tuple structs have positional semantics
        self.serializer.push_relation(
            "idx",
            vec![self.struct_id.clone(), self.index.to_string(), field_id],
            vec!["tuple_struct", "index", "atom"],
        );
        self.index += 1;
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(self.struct_id)
    }
}

pub(crate) struct TupleVariantSerializer<'a> {
    serializer: &'a mut JsonDataSerializer,
    variant_id: String,
    variant_type: String,
    index: usize,
}

impl<'a> SerializeTupleVariant for TupleVariantSerializer<'a> {
    type Ok = String;
    type Error = SerializationError;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        let field_id = value.serialize(&mut *self.serializer)?;
        self.serializer.push_relation(
            "idx",
            vec![self.variant_id.clone(), self.index.to_string(), field_id],
            vec![&self.variant_type, "index", "atom"],
        );
        self.index += 1;
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(self.variant_id)
    }
}

/// Emits `map_entry(map, key, value)` for `HashMap`/`BTreeMap`. Keys and values
/// are both full atoms, so either may be a complex type.
pub(crate) struct MapSerializer<'a> {
    serializer: &'a mut JsonDataSerializer,
    map_id: String,
    key_id: Option<String>,
}

impl<'a> SerializeMap for MapSerializer<'a> {
    type Ok = String;
    type Error = SerializationError;

    fn serialize_key<T: ?Sized + Serialize>(&mut self, key: &T) -> Result<(), Self::Error> {
        self.key_id = Some(key.serialize(&mut *self.serializer)?);
        Ok(())
    }

    fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        let value_id = value.serialize(&mut *self.serializer)?;
        if let Some(key_id) = self.key_id.take() {
            // map_entry(map, key, value) for associative collections
            self.serializer.push_relation(
                "map_entry",
                vec![self.map_id.clone(), key_id, value_id],
                vec!["map", "atom", "atom"],
            );
        }
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(self.map_id)
    }
}

/// Emits one relation per field, named after the field; the struct's atom type
/// is its own name rather than a generic "struct".
pub(crate) struct StructSerializer<'a> {
    serializer: &'a mut JsonDataSerializer,
    struct_id: String,
    struct_type: String,
}

impl<'a> SerializeStruct for StructSerializer<'a> {
    type Ok = String;
    type Error = SerializationError;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        let field_id = value.serialize(&mut *self.serializer)?;
        // Use field name as relation name: field_name(StructType, value)
        self.serializer.push_relation(
            key,
            vec![self.struct_id.clone(), field_id],
            vec![&self.struct_type, "atom"],
        );
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(self.struct_id)
    }
}

pub(crate) struct StructVariantSerializer<'a> {
    serializer: &'a mut JsonDataSerializer,
    variant_id: String,
    variant_type: String,
}

impl<'a> SerializeStructVariant for StructVariantSerializer<'a> {
    type Ok = String;
    type Error = SerializationError;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        let field_id = value.serialize(&mut *self.serializer)?;
        // Enum struct variants also use field names as relations
        self.serializer.push_relation(
            key,
            vec![self.variant_id.clone(), field_id],
            vec![&self.variant_type, "atom"],
        );
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(self.variant_id)
    }
}
