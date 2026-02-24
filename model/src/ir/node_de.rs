//! Manual Deserialize impl for NodeKind.
//!
//! WHY THIS EXISTS
//! ---------------
//! serde's derived Deserialize for externally-tagged enums uses
//! TaggedContentVisitor, which buffers the *entire* JSON object as a flat
//! Content tree and then re-scans it once per #[serde(default)] field.
//! When nested structs (Field, GenericParam, EnumVariant, …) share the key
//! "name" with the outer NodeKind variant, the re-scanner escapes the outer
//! object boundary and picks up a wrong string — e.g. NodeKind::Struct.name
//! receives a field's name instead of the struct's identifier.
//!
//! This manual impl deserializes each variant from its own bounded
//! serde_json::Map, which is *never* re-scanned across object boundaries.
//!
//! Variables:
//!   tag : String                     — the variant discriminant key
//!   obj : serde_json::Map            — the variant's own JSON object
//!   get : obj["key"] -> Value        — bounded lookup, no escape
//!
//! Equation:
//!   deserialize(NodeKind) =
//!     let {"<tag>": obj} = input in
//!     match tag { "Struct" => decode_struct(obj), … }
//!
//!   decode_struct(obj) =
//!     NodeKind::Struct {
//!       name        = obj["name"].as_str()          -- bounded to obj
//!       vis         = deserialize(obj["vis"])
//!       generics    = deserialize(obj["generics"])
//!       fields      = deserialize(obj["fields"])
//!       derives     = deserialize(obj["derives"])
//!       attrs       = obj.get("attrs").unwrap_or([])
//!       where_clauses = obj.get("where_clauses").unwrap_or([])
//!       struct_kind = obj.get("struct_kind").unwrap_or(Named)
//!     }

use serde::de::{self, Deserializer, MapAccess, Visitor};
use serde_json::Value;
use std::fmt;

use super::node::{
    Body, EnumVariant, Field, GenericParam, NodeKind, Param, TraitMethod,
};

// ── helpers ──────────────────────────────────────────────────────────────────

fn get_str<'a>(obj: &'a serde_json::Map<String, Value>, key: &str) -> Result<&'a str, String> {
    obj.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("NodeKind: missing or non-string field {:?}", key))
}

fn get_val(obj: &serde_json::Map<String, Value>, key: &str) -> Result<Value, String> {
    obj.get(key)
        .cloned()
        .ok_or_else(|| format!("NodeKind: missing field {:?}", key))
}

fn de<T: serde::de::DeserializeOwned>(v: Value) -> Result<T, String> {
    serde_json::from_value(v).map_err(|e| e.to_string())
}

fn opt_de<T: serde::de::DeserializeOwned>(
    obj: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<Option<T>, String> {
    match obj.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => serde_json::from_value(v.clone()).map(Some).map_err(|e| e.to_string()),
    }
}

fn default_de<T: serde::de::DeserializeOwned + Default>(
    obj: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<T, String> {
    match obj.get(key) {
        None | Some(Value::Null) => Ok(T::default()),
        Some(v) => serde_json::from_value(v.clone()).map_err(|e| e.to_string()),
    }
}

// ── variant decoders — each takes a *bounded* Map ────────────────────────────

fn decode_crate(obj: serde_json::Map<String, Value>) -> Result<NodeKind, String> {
    Ok(NodeKind::Crate {
        name:    get_str(&obj, "name")?.to_owned(),
        edition: get_str(&obj, "edition")?.to_owned(),
    })
}

fn decode_module(obj: serde_json::Map<String, Value>) -> Result<NodeKind, String> {
    Ok(NodeKind::Module {
        path:   get_str(&obj, "path")?.to_owned(),
        file:   get_str(&obj, "file")?.to_owned(),
        inline: default_de(&obj, "inline")?,
    })
}

fn decode_struct(obj: serde_json::Map<String, Value>) -> Result<NodeKind, String> {
    Ok(NodeKind::Struct {
        name:          get_str(&obj, "name")?.to_owned(),
        vis:           de(get_val(&obj, "vis")?)?,
        generics:      de::<Vec<GenericParam>>(get_val(&obj, "generics")?)?,
        fields:        de::<Vec<Field>>(get_val(&obj, "fields")?)?,
        derives:       de::<Vec<String>>(get_val(&obj, "derives")?)?,
        attrs:         default_de(&obj, "attrs")?,
        where_clauses: default_de(&obj, "where_clauses")?,
        struct_kind:   default_de(&obj, "struct_kind")?,
    })
}

fn decode_enum(obj: serde_json::Map<String, Value>) -> Result<NodeKind, String> {
    Ok(NodeKind::Enum {
        name:          get_str(&obj, "name")?.to_owned(),
        vis:           de(get_val(&obj, "vis")?)?,
        generics:      de::<Vec<GenericParam>>(get_val(&obj, "generics")?)?,
        variants:      de::<Vec<EnumVariant>>(get_val(&obj, "variants")?)?,
        derives:       de::<Vec<String>>(get_val(&obj, "derives")?)?,
        attrs:         default_de(&obj, "attrs")?,
        where_clauses: default_de(&obj, "where_clauses")?,
    })
}

fn decode_trait(obj: serde_json::Map<String, Value>) -> Result<NodeKind, String> {
    Ok(NodeKind::Trait {
        name:          get_str(&obj, "name")?.to_owned(),
        vis:           de(get_val(&obj, "vis")?)?,
        generics:      de::<Vec<GenericParam>>(get_val(&obj, "generics")?)?,
        methods:       de::<Vec<TraitMethod>>(get_val(&obj, "methods")?)?,
        attrs:         default_de(&obj, "attrs")?,
        where_clauses: default_de(&obj, "where_clauses")?,
        unsafe_:       default_de(&obj, "unsafe_")?,
    })
}

fn decode_impl(obj: serde_json::Map<String, Value>) -> Result<NodeKind, String> {
    Ok(NodeKind::Impl {
        for_struct:    get_str(&obj, "for_struct")?.to_owned(),
        for_trait:     opt_de(&obj, "for_trait")?,
        generics:      de::<Vec<GenericParam>>(get_val(&obj, "generics")?)?,
        attrs:         default_de(&obj, "attrs")?,
        where_clauses: default_de(&obj, "where_clauses")?,
        unsafe_:       default_de(&obj, "unsafe_")?,
    })
}

fn decode_function(obj: serde_json::Map<String, Value>) -> Result<NodeKind, String> {
    Ok(NodeKind::Function {
        name:          get_str(&obj, "name")?.to_owned(),
        vis:           de(get_val(&obj, "vis")?)?,
        generics:      de::<Vec<GenericParam>>(get_val(&obj, "generics")?)?,
        params:        de::<Vec<Param>>(get_val(&obj, "params")?)?,
        ret:           get_str(&obj, "ret")?.to_owned(),
        body:          de::<Body>(get_val(&obj, "body")?)?,
        attrs:         default_de(&obj, "attrs")?,
        where_clauses: default_de(&obj, "where_clauses")?,
        unsafe_:       default_de(&obj, "unsafe_")?,
        async_:        default_de(&obj, "async_")?,
    })
}

fn decode_method(obj: serde_json::Map<String, Value>) -> Result<NodeKind, String> {
    Ok(NodeKind::Method {
        name:          get_str(&obj, "name")?.to_owned(),
        vis:           de(get_val(&obj, "vis")?)?,
        generics:      de::<Vec<GenericParam>>(get_val(&obj, "generics")?)?,
        params:        de::<Vec<Param>>(get_val(&obj, "params")?)?,
        ret:           get_str(&obj, "ret")?.to_owned(),
        body:          de::<Body>(get_val(&obj, "body")?)?,
        attrs:         default_de(&obj, "attrs")?,
        where_clauses: default_de(&obj, "where_clauses")?,
        unsafe_:       default_de(&obj, "unsafe_")?,
        async_:        default_de(&obj, "async_")?,
    })
}

fn decode_const(obj: serde_json::Map<String, Value>) -> Result<NodeKind, String> {
    Ok(NodeKind::Const {
        name:  get_str(&obj, "name")?.to_owned(),
        vis:   de(get_val(&obj, "vis")?)?,
        ty:    get_str(&obj, "ty")?.to_owned(),
        value: get_str(&obj, "value")?.to_owned(),
        attrs: default_de(&obj, "attrs")?,
    })
}

fn decode_static(obj: serde_json::Map<String, Value>) -> Result<NodeKind, String> {
    Ok(NodeKind::Static {
        name:    get_str(&obj, "name")?.to_owned(),
        vis:     de(get_val(&obj, "vis")?)?,
        ty:      get_str(&obj, "ty")?.to_owned(),
        value:   get_str(&obj, "value")?.to_owned(),
        mutable: default_de(&obj, "mutable")?,
        attrs:   default_de(&obj, "attrs")?,
    })
}

fn decode_use(obj: serde_json::Map<String, Value>) -> Result<NodeKind, String> {
    Ok(NodeKind::Use {
        vis:   default_de(&obj, "vis")?,
        path:  get_str(&obj, "path")?.to_owned(),
        alias: opt_de(&obj, "alias")?,
        glob:  default_de(&obj, "glob")?,
    })
}

fn decode_type_ref(obj: serde_json::Map<String, Value>) -> Result<NodeKind, String> {
    Ok(NodeKind::TypeRef {
        name: get_str(&obj, "name")?.to_owned(),
    })
}

fn decode_type_alias(obj: serde_json::Map<String, Value>) -> Result<NodeKind, String> {
    Ok(NodeKind::TypeAlias {
        name:          get_str(&obj, "name")?.to_owned(),
        vis:           de(get_val(&obj, "vis")?)?,
        generics:      de::<Vec<GenericParam>>(get_val(&obj, "generics")?)?,
        ty:            get_str(&obj, "ty")?.to_owned(),
        attrs:         default_de(&obj, "attrs")?,
        where_clauses: default_de(&obj, "where_clauses")?,
    })
}

fn decode_lifetime(obj: serde_json::Map<String, Value>) -> Result<NodeKind, String> {
    Ok(NodeKind::Lifetime {
        name: get_str(&obj, "name")?.to_owned(),
    })
}

fn decode_extern_crate(obj: serde_json::Map<String, Value>) -> Result<NodeKind, String> {
    Ok(NodeKind::ExternCrate {
        name:  get_str(&obj, "name")?.to_owned(),
        alias: opt_de(&obj, "alias")?,
        vis:   default_de(&obj, "vis")?,
    })
}

fn decode_macro_call(obj: serde_json::Map<String, Value>) -> Result<NodeKind, String> {
    Ok(NodeKind::MacroCall {
        path:   get_str(&obj, "path")?.to_owned(),
        tokens: get_str(&obj, "tokens")?.to_owned(),
    })
}

// ── top-level Visitor ────────────────────────────────────────────────────────

struct NodeKindVisitor;

impl<'de> Visitor<'de> for NodeKindVisitor {
    type Value = NodeKind;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a NodeKind externally-tagged object")
    }

    // The externally-tagged JSON form is: { "Struct": { … } }
    // We deserialize the whole thing as a serde_json::Value first,
    // then dispatch to the bounded per-variant decoder.
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<NodeKind, A::Error> {
        // Collect into a Value so we can inspect the tag.
        let mut raw: serde_json::Map<String, Value> = serde_json::Map::new();
        while let Some((k, v)) = map.next_entry::<String, Value>()? {
            raw.insert(k, v);
        }

        if raw.len() != 1 {
            return Err(de::Error::custom(format!(
                "NodeKind: expected exactly 1 tag key, got {} keys: {:?}",
                raw.len(),
                raw.keys().collect::<Vec<_>>()
            )));
        }

        let (tag, content) = raw.into_iter().next().unwrap();

        // content must be an object for all struct variants
        let obj: serde_json::Map<String, Value> = match content {
            Value::Object(m) => m,
            Value::Null => serde_json::Map::new(), // unit-like
            other => {
                return Err(de::Error::custom(format!(
                    "NodeKind::{}: expected object, got {:?}",
                    tag, other
                )))
            }
        };

        match tag.as_str() {
            "Crate"       => decode_crate(obj),
            "Module"      => decode_module(obj),
            "Struct"      => decode_struct(obj),
            "Enum"        => decode_enum(obj),
            "Trait"       => decode_trait(obj),
            "Impl"        => decode_impl(obj),
            "Function"    => decode_function(obj),
            "Method"      => decode_method(obj),
            "Const"       => decode_const(obj),
            "Static"      => decode_static(obj),
            "Use"         => decode_use(obj),
            "TypeRef"     => decode_type_ref(obj),
            "TypeAlias"   => decode_type_alias(obj),
            "Lifetime"    => decode_lifetime(obj),
            "ExternCrate" => decode_extern_crate(obj),
            "MacroCall"   => decode_macro_call(obj),
            other => Err(format!("NodeKind: unknown variant {:?}", other)),
        }
        .map_err(de::Error::custom)
    }
}

impl<'de> de::Deserialize<'de> for NodeKind {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_map(NodeKindVisitor)
    }
}
