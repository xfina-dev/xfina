//! A map of decimals that serializes as JSON numbers.
//!
//! Every amount in the models is rendered as a number, and `rust_decimal`'s own
//! `float` adapter only covers a bare field. A keyed set of figures -- a rate
//! card's columns, a price row's publisher-specific extras -- needs this thin
//! wrapper around it.

use std::collections::BTreeMap;

use rust_decimal::Decimal;
use serde::de::{Deserialize, Deserializer};
use serde::ser::{SerializeMap, Serializer};

pub fn serialize<S: Serializer>(map: &BTreeMap<String, Decimal>, s: S) -> Result<S::Ok, S::Error> {
    #[derive(serde::Serialize)]
    struct AsFloat(#[serde(with = "rust_decimal::serde::float")] Decimal);

    let mut out = s.serialize_map(Some(map.len()))?;
    for (key, value) in map {
        out.serialize_entry(key, &AsFloat(*value))?;
    }
    out.end()
}

pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<BTreeMap<String, Decimal>, D::Error> {
    #[derive(serde::Deserialize)]
    struct AsFloat(#[serde(with = "rust_decimal::serde::float")] Decimal);

    let raw = BTreeMap::<String, AsFloat>::deserialize(d)?;
    Ok(raw.into_iter().map(|(k, v)| (k, v.0)).collect())
}
