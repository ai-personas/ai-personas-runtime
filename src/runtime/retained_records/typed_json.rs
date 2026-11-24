//! Reuse exact, completely decoded reference and policy JSON within one record
//! pool load. Only the two typed newtype visitors can receive these values.
//! Other JSON, container order and duplicate members retain native decoding.
use super::super::{f, ScopedPolicy, SHARED_SCOPED_POLICY};
use serde::de::{
    self, DeserializeOwned, DeserializeSeed, EnumAccess, MapAccess, SeqAccess, VariantAccess,
    Visitor,
};
use serde::{Deserialize, Deserializer};
use serde_json_raw::value::RawValue;
use std::{collections::HashMap, fmt};

#[derive(Default)]
pub(super) struct Atoms {
    references: HashMap<String, f::Reference>,
    policies: HashMap<String, ScopedPolicy>,
}

impl Atoms {
    pub(super) fn decode<T: DeserializeOwned>(&mut self, bytes: &[u8]) -> serde_json::Result<T> {
        let mut native = serde_json_raw::Deserializer::from_slice(bytes);
        native.disable_recursion_limit();
        let decoded = T::deserialize(Reader {
            native: &mut native,
            atoms: self,
        })
        .and_then(|value| {
            native.end()?;
            Ok(value)
        });
        if let Ok(value) = decoded {
            return Ok(value);
        }
        // Raw-value scanning can encounter malformed later text before a typed
        // visitor's earlier error. Preserve the complete original result and
        // error position on any unsuccessful accelerated decode.
        let mut native = serde_json::Deserializer::from_slice(bytes);
        native.disable_recursion_limit();
        let value = T::deserialize(&mut native)?;
        native.end()?;
        Ok(value)
    }
}

struct Reader<'a, D> {
    native: D,
    atoms: &'a mut Atoms,
}

macro_rules! scalar {
    ($method:ident $(, $arg:ident: $kind:ty)*) => {
        fn $method<V: Visitor<'de>>(self, $($arg: $kind,)* visitor: V) -> Result<V::Value, D::Error> {
            self.native.$method($($arg,)* visitor)
        }
    };
}
macro_rules! container {
    ($method:ident $(, $arg:ident: $kind:ty)*) => {
        fn $method<V: Visitor<'de>>(self, $($arg: $kind,)* visitor: V) -> Result<V::Value, D::Error> {
            self.native.$method($($arg,)* Forward { visitor, atoms: self.atoms })
        }
    };
}

impl<'de, D: Deserializer<'de>> Deserializer<'de> for Reader<'_, D> {
    type Error = D::Error;
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
    container!(deserialize_any);
    container!(deserialize_option);
    container!(deserialize_seq);
    container!(deserialize_tuple, len: usize);
    container!(deserialize_tuple_struct, name: &'static str, len: usize);
    container!(deserialize_map);
    container!(deserialize_struct, name: &'static str, fields: &'static [&'static str]);
    container!(deserialize_enum, name: &'static str, variants: &'static [&'static str]);

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, D::Error> {
        if name == f::SHARED_REFERENCE {
            let raw = <&RawValue>::deserialize(self.native)?.get();
            let value = match self.atoms.references.get(raw) {
                Some(value) => value.clone(),
                None => {
                    let value =
                        serde_json::from_str::<f::Reference>(raw).map_err(de::Error::custom)?;
                    self.atoms.references.insert(raw.into(), value.clone());
                    value
                }
            };
            return crate::shared_values::supply(value.shared_value(), || visitor.visit_unit());
        }
        if name == SHARED_SCOPED_POLICY {
            let raw = <&RawValue>::deserialize(self.native)?.get();
            let value = match self.atoms.policies.get(raw) {
                Some(value) => value.clone(),
                None => {
                    let value =
                        serde_json::from_str::<ScopedPolicy>(raw).map_err(de::Error::custom)?;
                    self.atoms.policies.insert(raw.into(), value.clone());
                    value
                }
            };
            return crate::shared_values::supply(value.0.clone(), || visitor.visit_unit());
        }
        self.native.deserialize_newtype_struct(
            name,
            Forward {
                visitor,
                atoms: self.atoms,
            },
        )
    }

    fn is_human_readable(&self) -> bool {
        self.native.is_human_readable()
    }
}

struct Forward<'a, V> {
    visitor: V,
    atoms: &'a mut Atoms,
}

macro_rules! visit_scalar {
    ($method:ident $(, $arg:ident: $kind:ty)*) => {
        fn $method<E: de::Error>(self, $($arg: $kind,)*) -> Result<Self::Value, E> {
            self.visitor.$method($($arg,)*)
        }
    };
}

impl<'de, V: Visitor<'de>> Visitor<'de> for Forward<'_, V> {
    type Value = V::Value;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.visitor.expecting(f)
    }
    visit_scalar!(visit_bool, value: bool);
    visit_scalar!(visit_i8, value: i8);
    visit_scalar!(visit_i16, value: i16);
    visit_scalar!(visit_i32, value: i32);
    visit_scalar!(visit_i64, value: i64);
    visit_scalar!(visit_i128, value: i128);
    visit_scalar!(visit_u8, value: u8);
    visit_scalar!(visit_u16, value: u16);
    visit_scalar!(visit_u32, value: u32);
    visit_scalar!(visit_u64, value: u64);
    visit_scalar!(visit_u128, value: u128);
    visit_scalar!(visit_f32, value: f32);
    visit_scalar!(visit_f64, value: f64);
    visit_scalar!(visit_char, value: char);
    visit_scalar!(visit_str, value: &str);
    visit_scalar!(visit_borrowed_str, value: &'de str);
    visit_scalar!(visit_string, value: String);
    visit_scalar!(visit_bytes, value: &[u8]);
    visit_scalar!(visit_borrowed_bytes, value: &'de [u8]);
    visit_scalar!(visit_byte_buf, value: Vec<u8>);
    visit_scalar!(visit_none);
    visit_scalar!(visit_unit);

    fn visit_some<D: Deserializer<'de>>(self, native: D) -> Result<Self::Value, D::Error> {
        self.visitor.visit_some(Reader {
            native,
            atoms: self.atoms,
        })
    }
    fn visit_newtype_struct<D: Deserializer<'de>>(
        self,
        native: D,
    ) -> Result<Self::Value, D::Error> {
        self.visitor.visit_newtype_struct(Reader {
            native,
            atoms: self.atoms,
        })
    }
    fn visit_seq<A: SeqAccess<'de>>(self, native: A) -> Result<Self::Value, A::Error> {
        self.visitor.visit_seq(Access {
            native,
            atoms: self.atoms,
        })
    }
    fn visit_map<A: MapAccess<'de>>(self, native: A) -> Result<Self::Value, A::Error> {
        self.visitor.visit_map(Access {
            native,
            atoms: self.atoms,
        })
    }
    fn visit_enum<A: EnumAccess<'de>>(self, native: A) -> Result<Self::Value, A::Error> {
        self.visitor.visit_enum(Access {
            native,
            atoms: self.atoms,
        })
    }
}

struct Seed<'a, S> {
    seed: S,
    atoms: &'a mut Atoms,
}
impl<'de, S: DeserializeSeed<'de>> DeserializeSeed<'de> for Seed<'_, S> {
    type Value = S::Value;
    fn deserialize<D: Deserializer<'de>>(self, native: D) -> Result<Self::Value, D::Error> {
        self.seed.deserialize(Reader {
            native,
            atoms: self.atoms,
        })
    }
}

struct Access<'a, A> {
    native: A,
    atoms: &'a mut Atoms,
}
impl<'de, A: SeqAccess<'de>> SeqAccess<'de> for Access<'_, A> {
    type Error = A::Error;
    fn next_element_seed<S: DeserializeSeed<'de>>(
        &mut self,
        seed: S,
    ) -> Result<Option<S::Value>, A::Error> {
        self.native.next_element_seed(Seed {
            seed,
            atoms: self.atoms,
        })
    }
    fn size_hint(&self) -> Option<usize> {
        self.native.size_hint()
    }
}
impl<'de, A: MapAccess<'de>> MapAccess<'de> for Access<'_, A> {
    type Error = A::Error;
    fn next_key_seed<S: DeserializeSeed<'de>>(
        &mut self,
        seed: S,
    ) -> Result<Option<S::Value>, A::Error> {
        self.native.next_key_seed(seed)
    }
    fn next_value_seed<S: DeserializeSeed<'de>>(&mut self, seed: S) -> Result<S::Value, A::Error> {
        self.native.next_value_seed(Seed {
            seed,
            atoms: self.atoms,
        })
    }
    fn size_hint(&self) -> Option<usize> {
        self.native.size_hint()
    }
}
impl<'a, 'de, A: EnumAccess<'de>> EnumAccess<'de> for Access<'a, A> {
    type Error = A::Error;
    type Variant = Access<'a, A::Variant>;
    fn variant_seed<S: DeserializeSeed<'de>>(
        self,
        seed: S,
    ) -> Result<(S::Value, Self::Variant), A::Error> {
        let (value, native) = self.native.variant_seed(seed)?;
        Ok((
            value,
            Access {
                native,
                atoms: self.atoms,
            },
        ))
    }
}
impl<'de, A: VariantAccess<'de>> VariantAccess<'de> for Access<'_, A> {
    type Error = A::Error;
    fn unit_variant(self) -> Result<(), A::Error> {
        self.native.unit_variant()
    }
    fn newtype_variant_seed<S: DeserializeSeed<'de>>(self, seed: S) -> Result<S::Value, A::Error> {
        self.native.newtype_variant_seed(Seed {
            seed,
            atoms: self.atoms,
        })
    }
    fn tuple_variant<V: Visitor<'de>>(self, len: usize, visitor: V) -> Result<V::Value, A::Error> {
        self.native.tuple_variant(
            len,
            Forward {
                visitor,
                atoms: self.atoms,
            },
        )
    }
    fn struct_variant<V: Visitor<'de>>(
        self,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, A::Error> {
        self.native.struct_variant(
            fields,
            Forward {
                visitor,
                atoms: self.atoms,
            },
        )
    }
}
