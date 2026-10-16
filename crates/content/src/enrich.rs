//! 40kdc enrichment: ability name → structured effect tree.

use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

/// Faction catalogue name as the roster prints it → 40kdc faction slug.
pub fn faction_slug(catalogue: &str) -> Option<&'static str> {
    let c = catalogue.to_ascii_lowercase();
    let table: &[(&str, &str)] = &[
        ("adeptus mechanicus", "adeptus-mechanicus"),
        ("adeptus custodes", "adeptus-custodes"),
        ("necrons", "necrons"),
        ("adepta sororitas", "adepta-sororitas"),
        ("astra militarum", "astra-militarum"),
        ("space marines", "adeptus-astartes"),
        ("blood angels", "blood-angels"),
        ("dark angels", "dark-angels"),
        ("space wolves", "space-wolves"),
        ("grey knights", "grey-knights"),
        ("imperial knights", "imperial-knights"),
        ("chaos knights", "chaos-knights"),
        ("chaos space marines", "chaos-space-marines"),
        ("chaos daemons", "chaos-daemons"),
        ("death guard", "death-guard"),
        ("thousand sons", "thousand-sons"),
        ("world eaters", "world-eaters"),
        ("emperor's children", "emperors-children"),
        ("aeldari", "aeldari"),
        ("craftworlds", "aeldari"),
        ("drukhari", "drukhari"),
        ("orks", "orks"),
        ("tyranids", "tyranids"),
        ("genestealer cults", "genestealer-cults"),
        ("t'au empire", "tau-empire"),
        ("leagues of votann", "leagues-of-votann"),
        ("agents of the imperium", "agents-of-the-imperium"),
    ];
    table.iter().find(|(k, _)| c.contains(k)).map(|(_, v)| *v)
}

/// Load `data/enrichment/<faction>/abilities.json` plus the shared `_core`
/// file: whole entries keyed by lower-cased ability name.
pub fn load_abilities(root: &Path, faction: &str) -> Result<HashMap<String, Value>> {
    let mut map = HashMap::new();
    for dir in ["_core", faction] {
        let path = root.join("data/40kdc-data/data/enrichment").join(dir).join("abilities.json");
        if !path.exists() {
            continue;
        }
        let text = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        index_abilities(&text, &mut map).with_context(|| format!("parsing {}", path.display()))?;
    }
    Ok(map)
}

/// Index the text of one abilities.json into `map` (first entry per name wins).
pub fn index_abilities(text: &str, map: &mut HashMap<String, Value>) -> Result<()> {
    let list: Vec<Value> = serde_json::from_str(text)?;
    for a in list {
        if let Some(name) = a.get("name").and_then(Value::as_str) {
            map.entry(name.to_ascii_lowercase()).or_insert(a);
        }
    }
    Ok(())
}
