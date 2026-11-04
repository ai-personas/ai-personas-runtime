//! A borrowed view of validated storage references at known Exposure fields.
//!
//! Native serde_json access still decodes keys, scalars and enum discriminants.
//! Child seeds receive the corresponding borrowed value from the same immutable
//! container, with only the validated Exposure locations resolved through the
//! pool. Arbitrary authored JSON is not searched for reference-shaped objects.
use serde::de::{self, DeserializeSeed, EnumAccess, MapAccess, SeqAccess, VariantAccess, Visitor};
use serde::Deserializer;
use serde_json::Value;
use std::collections::HashMap;
use std::fmt;

// Atoms contain only complete typed reference or policy fields. They cannot
// contain another storage reference, so their children use native decoding.
#[derive(Clone)]
pub(super) enum Replacement<'de> {
    Manifest(&'de Value),
    Reference(&'de Value, super::f::Reference),
    Policy(&'de Value, super::ScopedPolicy),
}

struct Context<'de> {
    root: &'de Value,
    replacements: HashMap<usize, Replacement<'de>>,
}

#[derive(Clone, Copy)]
struct View<'a, 'de> {
    value: &'de Value,
    context: &'a Context<'de>,
    atom: Option<&'a Replacement<'de>>,
}

impl<'a, 'de> View<'a, 'de> {
    fn child(self, value: &'de Value) -> Self {
        let (value, atom) = match self.context.replacements.get(&location(value)) {
            Some(Replacement::Manifest(value)) => (*value, None),
            Some(atom @ (Replacement::Reference(value, _) | Replacement::Policy(value, _))) => (*value, Some(atom)),
            None => (value, None),
        };
        Self { value, context: self.context, atom }
    }
}

// An address is only an identity key during this borrow. No pointer is ever
// dereferenced, persisted or used after the stored JSON can move or change.
pub(super) fn location(value: &Value) -> usize {
    value as *const Value as usize
}

pub(super) struct Decoder<'de>(Context<'de>);

impl<'de> Decoder<'de> {
    pub(super) fn new(root: &'de Value, replacements: HashMap<usize, Replacement<'de>>) -> Self {
        Self(Context { root, replacements })
    }
    pub(super) fn state(&self) -> serde_json::Result<super::State> {
        super::ExpandedState::deserialize(View { value: self.0.root, context: &self.0, atom: None })
    }
    pub(super) fn record<T: serde::Deserialize<'de>>(&self, value: &'de Value) -> serde_json::Result<T> {
        T::deserialize(View { value, context: &self.0, atom: None })
    }
}

macro_rules! scalar {
    ($method:ident $(, $arg:ident: $kind:ty)*) => {
        fn $method<V: Visitor<'de>>(self, $($arg: $kind,)* visitor: V) -> serde_json::Result<V::Value> {
            self.value.$method($($arg,)* visitor)
        }
    };
}

macro_rules! container {
    ($method:ident $(, $arg:ident: $kind:ty)*) => {
        fn $method<V: Visitor<'de>>(self, $($arg: $kind,)* visitor: V) -> serde_json::Result<V::Value> {
            if self.atom.is_some() {
                self.value.$method($($arg,)* visitor)
            } else {
                self.value.$method($($arg,)* Containers { visitor, source: self })
            }
        }
    };
}

impl<'a, 'de> Deserializer<'de> for View<'a, 'de> {
    type Error = serde_json::Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> serde_json::Result<V::Value> {
        if self.atom.is_some() {
            return self.value.deserialize_any(visitor);
        }
        match self.value {
            Value::Array(_) | Value::Object(_) => self.value.deserialize_any(Containers {
                visitor,
                source: self,
            }),
            // Number deserializers can expose synthetic maps with additional
            // dependency features. They contain no indexed Exposure locations.
            _ => self.value.deserialize_any(visitor),
        }
    }

    scalar!(deserialize_bool);
    scalar!(deserialize_i8);
    scalar!(deserialize_i16);
    scalar!(deserialize_i32);
    scalar!(deserialize_i64);
    scalar!(deserialize_i128);
    scalar!(deserialize_u8);
    scalar!(deserialize_u16);
    scalar!(deserialize_u32);
    scalar!(deserialize_u64);
    scalar!(deserialize_u128);
    scalar!(deserialize_f32);
    scalar!(deserialize_f64);
    scalar!(deserialize_char);
    scalar!(deserialize_str);
    scalar!(deserialize_string);
    scalar!(deserialize_bytes);
    scalar!(deserialize_byte_buf);
    scalar!(deserialize_unit);
    scalar!(deserialize_unit_struct, name: &'static str);
    scalar!(deserialize_identifier);
    scalar!(deserialize_ignored_any);

    container!(deserialize_seq);
    container!(deserialize_tuple, len: usize);
    container!(deserialize_tuple_struct, name: &'static str, len: usize);
    container!(deserialize_map);
    container!(deserialize_struct, name: &'static str, fields: &'static [&'static str]);

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> serde_json::Result<V::Value> {
        if self.atom.is_some() || self.value.is_null() {
            self.value.deserialize_option(visitor)
        } else {
            visitor.visit_some(self)
        }
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> serde_json::Result<V::Value> {
        match (name, self.atom) {
            (super::f::SHARED_REFERENCE, Some(Replacement::Reference(_, value))) => {
                return crate::shared_values::supply(value.shared_value(), || visitor.visit_unit());
            }
            (super::SHARED_SCOPED_POLICY, Some(Replacement::Policy(_, value))) => {
                return crate::shared_values::supply(value.0.clone(), || visitor.visit_unit());
            }
            _ => (),
        }
        if self.atom.is_some() {
            return self.value.deserialize_newtype_struct(name, visitor);
        }
        self.value.deserialize_newtype_struct(
            name,
            Newtype {
                visitor,
                source: self,
            },
        )
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> serde_json::Result<V::Value> {
        if self.atom.is_some() {
            return self.value.deserialize_enum(name, variants, visitor);
        }
        self.value.deserialize_enum(
            name,
            variants,
            Enumeration {
                visitor,
                source: self,
            },
        )
    }
}

struct Child<'a, 'de, S> {
    seed: S,
    source: View<'a, 'de>,
}

impl<'a, 'de, S: DeserializeSeed<'de>> DeserializeSeed<'de> for Child<'a, 'de, S> {
    type Value = S::Value;

    fn deserialize<D: Deserializer<'de>>(self, _native_child: D) -> Result<Self::Value, D::Error> {
        // The native access has consumed this same child. Decode its borrowed
        // view, preserving native key parsing and container consumption checks.
        self.seed
            .deserialize(self.source)
            .map_err(de::Error::custom)
    }
}

struct Containers<'a, 'de, V> {
    visitor: V,
    source: View<'a, 'de>,
}

impl<'a, 'de, V: Visitor<'de>> Visitor<'de> for Containers<'a, 'de, V> {
    type Value = V::Value;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.visitor.expecting(f)
    }

    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        self.visitor.visit_unit()
    }

    fn visit_seq<A: SeqAccess<'de>>(self, native: A) -> Result<Self::Value, A::Error> {
        let values = self
            .source
            .value
            .as_array()
            .ok_or_else(|| de::Error::custom("borrowed storage sequence mismatch"))?;
        self.visitor.visit_seq(Sequence {
            native,
            values: values.iter(),
            source: self.source,
        })
    }

    fn visit_map<A: MapAccess<'de>>(self, native: A) -> Result<Self::Value, A::Error> {
        let fields = self
            .source
            .value
            .as_object()
            .ok_or_else(|| de::Error::custom("borrowed storage map mismatch"))?;
        self.visitor.visit_map(Fields {
            native,
            fields: fields.iter(),
            value: None,
            source: self.source,
            omit_pool: std::ptr::eq(self.source.value, self.source.context.root),
        })
    }
}

struct Sequence<'a, 'de, A> {
    native: A,
    values: std::slice::Iter<'de, Value>,
    source: View<'a, 'de>,
}

impl<'a, 'de, A: SeqAccess<'de>> SeqAccess<'de> for Sequence<'a, 'de, A> {
    type Error = A::Error;

    fn next_element_seed<S: DeserializeSeed<'de>>(
        &mut self,
        seed: S,
    ) -> Result<Option<S::Value>, A::Error> {
        match self.values.next() {
            Some(value) => self
                .native
                .next_element_seed(Child {
                    seed,
                    source: self.source.child(value),
                })?
                .map(Some)
                .ok_or_else(|| de::Error::custom("borrowed storage sequence mismatch")),
            None => match self.native.next_element_seed(seed)? {
                None => Ok(None),
                Some(_) => Err(de::Error::custom("borrowed storage sequence mismatch")),
            },
        }
    }

    fn size_hint(&self) -> Option<usize> {
        self.native.size_hint()
    }
}

struct Fields<'a, 'de, A> {
    native: A,
    fields: serde_json::map::Iter<'de>,
    value: Option<&'de Value>,
    source: View<'a, 'de>,
    omit_pool: bool,
}

impl<'a, 'de, A: MapAccess<'de>> MapAccess<'de> for Fields<'a, 'de, A> {
    type Error = A::Error;

    fn next_key_seed<S: DeserializeSeed<'de>>(
        &mut self,
        seed: S,
    ) -> Result<Option<S::Value>, A::Error> {
        loop {
            match self.fields.next() {
                Some((key, _)) if self.omit_pool
                    && (key == super::POOL || key == super::retained_records::POOL) => {
                    if self
                        .native
                        .next_entry::<de::IgnoredAny, de::IgnoredAny>()?
                        .is_none()
                    {
                        return Err(de::Error::custom("borrowed storage map mismatch"));
                    }
                }
                Some((_, value)) => {
                    self.value = Some(value);
                    // Native serde_json key deserialization supports its usual
                    // numeric and newtype map keys as well as string keys.
                    return self
                        .native
                        .next_key_seed(seed)?
                        .map(Some)
                        .ok_or_else(|| de::Error::custom("borrowed storage map mismatch"));
                }
                None => {
                    return match self.native.next_key_seed(seed)? {
                        None => Ok(None),
                        Some(_) => Err(de::Error::custom("borrowed storage map mismatch")),
                    }
                }
            }
        }
    }

    fn next_value_seed<S: DeserializeSeed<'de>>(&mut self, seed: S) -> Result<S::Value, A::Error> {
        let value = self
            .value
            .take()
            .ok_or_else(|| de::Error::custom("borrowed storage value absent"))?;
        self.native.next_value_seed(Child {
            seed,
            source: self.source.child(value),
        })
    }

    fn size_hint(&self) -> Option<usize> {
        // Either storage pool can remain among the native entries. Omitting a
        // hint avoids claiming an exact size before those fields are skipped.
        None
    }
}

struct Newtype<'a, 'de, V> {
    visitor: V,
    source: View<'a, 'de>,
}

impl<'a, 'de, V: Visitor<'de>> Visitor<'de> for Newtype<'a, 'de, V> {
    type Value = V::Value;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.visitor.expecting(f)
    }

    fn visit_newtype_struct<D: Deserializer<'de>>(
        self,
        _native: D,
    ) -> Result<Self::Value, D::Error> {
        self.visitor
            .visit_newtype_struct(self.source)
            .map_err(de::Error::custom)
    }

    fn visit_map<A: MapAccess<'de>>(self, native: A) -> Result<Self::Value, A::Error> {
        // serde_json raw-value newtypes use a synthetic map. It is opaque
        // authored data, not a container of indexed typed Exposure fields.
        self.visitor.visit_map(native)
    }
}

struct Enumeration<'a, 'de, V> {
    visitor: V,
    source: View<'a, 'de>,
}

impl<'a, 'de, V: Visitor<'de>> Visitor<'de> for Enumeration<'a, 'de, V> {
    type Value = V::Value;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.visitor.expecting(f)
    }

    fn visit_enum<A: EnumAccess<'de>>(self, native: A) -> Result<Self::Value, A::Error> {
        let value = self
            .source
            .value
            .as_object()
            .and_then(|fields| fields.values().next());
        self.visitor.visit_enum(Variants {
            native,
            value,
            source: self.source,
        })
    }
}

struct Variants<'a, 'de, A> {
    native: A,
    value: Option<&'de Value>,
    source: View<'a, 'de>,
}

impl<'a, 'de, A: EnumAccess<'de>> EnumAccess<'de> for Variants<'a, 'de, A> {
    type Error = A::Error;
    type Variant = Variants<'a, 'de, A::Variant>;

    fn variant_seed<S: DeserializeSeed<'de>>(
        self,
        seed: S,
    ) -> Result<(S::Value, Self::Variant), A::Error> {
        let (value, native) = self.native.variant_seed(seed)?;
        Ok((
            value,
            Variants {
                native,
                value: self.value,
                source: self.source,
            },
        ))
    }
}

impl<'a, 'de, A: VariantAccess<'de>> VariantAccess<'de> for Variants<'a, 'de, A> {
    type Error = A::Error;

    fn unit_variant(self) -> Result<(), A::Error> {
        self.native.unit_variant()
    }

    fn newtype_variant_seed<S: DeserializeSeed<'de>>(self, seed: S) -> Result<S::Value, A::Error> {
        match self.value {
            Some(value) => self.native.newtype_variant_seed(Child {
                seed,
                source: self.source.child(value),
            }),
            None => self.native.newtype_variant_seed(seed),
        }
    }

    fn tuple_variant<V: Visitor<'de>>(self, len: usize, visitor: V) -> Result<V::Value, A::Error> {
        match self.value {
            Some(value) => self.native.tuple_variant(
                len,
                Containers {
                    visitor,
                    source: self.source.child(value),
                },
            ),
            None => self.native.tuple_variant(len, visitor),
        }
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, A::Error> {
        match self.value {
            Some(value) => self.native.struct_variant(
                fields,
                Containers {
                    visitor,
                    source: self.source.child(value),
                },
            ),
            None => self.native.struct_variant(fields, visitor),
        }
    }
}
