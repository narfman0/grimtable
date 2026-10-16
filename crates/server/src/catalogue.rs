//! Faction catalogues and ability indexes, built lazily from `data/` on
//! first request and kept in memory for the life of the process.

use crate::error::{ApiError, ApiResult};
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::Json;
use content::catalogue::{self as cat, Catalogue};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

pub type Abilities = HashMap<String, Value>;
/// `(slug, name)` for every faction with core data.
pub type Factions = Vec<(String, String)>;

pub struct CatEntry {
    pub cat: Catalogue,
    /// Serialised once; the catalogue is large.
    pub json: String,
}

pub struct Cache {
    root: PathBuf,
    cats: RwLock<HashMap<String, Arc<CatEntry>>>,
    abilities: RwLock<HashMap<String, Arc<Abilities>>>,
    factions: RwLock<Option<Arc<Factions>>>,
}

fn valid_slug(slug: &str) -> bool {
    !slug.is_empty() && slug.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

impl Cache {
    pub fn new(root: PathBuf) -> Cache {
        Cache { root, cats: RwLock::new(HashMap::new()), abilities: RwLock::new(HashMap::new()), factions: RwLock::new(None) }
    }

    /// Every faction with core data: `(slug, name)`.
    pub fn factions(&self) -> ApiResult<Arc<Factions>> {
        if let Some(f) = self.factions.read().unwrap().clone() {
            return Ok(f);
        }
        let list = Arc::new(cat::factions(&self.root)?);
        *self.factions.write().unwrap() = Some(list.clone());
        Ok(list)
    }

    pub fn has_faction(&self, slug: &str) -> ApiResult<bool> {
        Ok(valid_slug(slug) && self.factions()?.iter().any(|(s, _)| s == slug))
    }

    pub async fn catalogue(&self, slug: &str) -> ApiResult<Arc<CatEntry>> {
        if !self.has_faction(slug)? {
            return Err(ApiError::not_found(format!("no faction {slug}")));
        }
        if let Some(c) = self.cats.read().unwrap().get(slug).cloned() {
            return Ok(c);
        }
        let root = self.root.clone();
        let slug_owned = slug.to_string();
        let entry = tokio::task::spawn_blocking(move || -> anyhow::Result<CatEntry> {
            let bs = cat::bsdata_file(&slug_owned).and_then(|f| match cat::bsdata_points(&root, f) {
                Ok(b) => Some(b),
                Err(e) => {
                    tracing::warn!("BSData points for {slug_owned}: {e:#}");
                    None
                }
            });
            let c = cat::build(&root, &slug_owned, bs.as_ref())?;
            let json = serde_json::to_string(&c)?;
            Ok(CatEntry { cat: c, json })
        })
        .await??;
        tracing::info!(faction = slug, datasheets = entry.cat.units.len(), "catalogue built");
        let entry = Arc::new(entry);
        self.cats.write().unwrap().insert(slug.to_string(), entry.clone());
        Ok(entry)
    }

    /// The 40kdc enrichment index for a faction (`_core` is always merged in).
    pub async fn abilities(&self, slug: &str) -> ApiResult<Arc<Abilities>> {
        if !valid_slug(slug) {
            return Err(ApiError::not_found(format!("no faction {slug}")));
        }
        if let Some(a) = self.abilities.read().unwrap().get(slug).cloned() {
            return Ok(a);
        }
        let root = self.root.clone();
        let slug_owned = slug.to_string();
        let map = tokio::task::spawn_blocking(move || content::enrich::load_abilities(&root, &slug_owned)).await??;
        let map = Arc::new(map);
        self.abilities.write().unwrap().insert(slug.to_string(), map.clone());
        Ok(map)
    }

    /// Text of the faction's `detachments.json`, if it has one.
    pub fn detachments_json(&self, slug: &str) -> Option<String> {
        if !valid_slug(slug) {
            return None;
        }
        std::fs::read_to_string(self.root.join("data/40kdc-data/data/core").join(slug).join("detachments.json")).ok()
    }
}

pub async fn list(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let factions = state.cache.factions()?;
    Ok(Json(factions.iter().map(|(slug, name)| json!({ "faction_id": slug, "name": name })).collect()))
}

pub async fn get_one(State(state): State<AppState>, Path(faction_id): Path<String>) -> ApiResult<Response> {
    let entry = state.cache.catalogue(&faction_id).await?;
    Ok(([(header::CONTENT_TYPE, "application/json")], entry.json.clone()).into_response())
}
