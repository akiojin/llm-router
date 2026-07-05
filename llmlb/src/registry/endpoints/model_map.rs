//! model→endpoint のルックアップキー生成とインメモリインデックス保守
//!
//! arch-review [H6]: registry/endpoints.rs から凝集した索引ロジックを分離。

use crate::types::endpoint::EndpointModel;
use std::collections::HashMap;
use uuid::Uuid;

pub(super) fn model_lookup_keys(model_id: &str) -> Vec<String> {
    let mut keys = vec![model_id.to_string()];
    if let Some(mapping) = crate::models::mapping::find_mapping(model_id) {
        if !keys.iter().any(|key| key == mapping.canonical) {
            keys.push(mapping.canonical.to_string());
        }
        for alias in mapping.aliases {
            if !keys.iter().any(|key| key == alias.name) {
                keys.push(alias.name.to_string());
            }
        }
    }
    keys
}

pub(super) fn endpoint_model_lookup_keys(model: &EndpointModel) -> Vec<String> {
    let mut keys = model_lookup_keys(&model.model_id);
    if let Some(canonical) = model.canonical_name.as_deref() {
        for key in model_lookup_keys(canonical) {
            if !keys.iter().any(|existing| existing == &key) {
                keys.push(key);
            }
        }
    }
    keys
}

pub(super) fn insert_model_mapping(
    model_map: &mut HashMap<String, Vec<Uuid>>,
    model: &EndpointModel,
    endpoint_id: Uuid,
) {
    for key in endpoint_model_lookup_keys(model) {
        let endpoints = model_map.entry(key).or_default();
        if !endpoints.contains(&endpoint_id) {
            endpoints.push(endpoint_id);
        }
    }
}

pub(super) fn remove_model_mapping(
    model_map: &mut HashMap<String, Vec<Uuid>>,
    model: &EndpointModel,
    endpoint_id: Uuid,
) {
    for key in endpoint_model_lookup_keys(model) {
        let remove_key = if let Some(endpoints) = model_map.get_mut(&key) {
            endpoints.retain(|id| *id != endpoint_id);
            endpoints.is_empty()
        } else {
            false
        };

        if remove_key {
            model_map.remove(&key);
        }
    }
}
