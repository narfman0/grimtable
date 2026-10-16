//! Maps raw weapon keyword text onto the engine's `WeaponAbility` enum.

use engine::attack::WeaponAbility;
use engine::dice::DiceExpr;

use crate::{normalize, parse_plus, strip_prefix_ci};

/// Parses one keyword as a roster prints it, e.g. `Rapid Fire 2`,
/// `Sustained Hits D3`, `Anti-Vehicle 4+`. Matching is case-insensitive;
/// the `Anti` keyword text is kept as written. Returns `None` for anything
/// the engine does not model.
pub fn parse_weapon_keyword(s: &str) -> Option<WeaponAbility> {
    let s = normalize(s);
    let lower = s.to_ascii_lowercase();
    let plain = match lower.as_str() {
        "assault" => Some(WeaponAbility::Assault),
        "blast" => Some(WeaponAbility::Blast),
        "devastating wounds" => Some(WeaponAbility::DevastatingWounds),
        "extra attacks" => Some(WeaponAbility::ExtraAttacks),
        "hazardous" => Some(WeaponAbility::Hazardous),
        "heavy" => Some(WeaponAbility::Heavy),
        "ignores cover" => Some(WeaponAbility::IgnoresCover),
        "indirect fire" => Some(WeaponAbility::IndirectFire),
        "lance" => Some(WeaponAbility::Lance),
        "lethal hits" => Some(WeaponAbility::LethalHits),
        "pistol" => Some(WeaponAbility::Pistol),
        "precision" => Some(WeaponAbility::Precision),
        "psychic" => Some(WeaponAbility::Psychic),
        "torrent" => Some(WeaponAbility::Torrent),
        "twin-linked" | "twin linked" => Some(WeaponAbility::TwinLinked),
        _ => None,
    };
    if plain.is_some() {
        return plain;
    }
    if let Some(rest) = strip_prefix_ci(&s, "melta ") {
        return rest.trim().parse().ok().map(WeaponAbility::Melta);
    }
    if let Some(rest) = strip_prefix_ci(&s, "rapid fire ") {
        return rest.trim().parse().ok().map(WeaponAbility::RapidFire);
    }
    if let Some(rest) = strip_prefix_ci(&s, "sustained hits ") {
        return DiceExpr::parse(rest).ok().map(WeaponAbility::SustainedHits);
    }
    if let Some(rest) = strip_prefix_ci(&s, "anti-") {
        let (keyword, threshold) = rest.trim().rsplit_once(' ')?;
        return Some(WeaponAbility::Anti {
            keyword: keyword.trim().to_string(),
            threshold: parse_plus(threshold)?,
        });
    }
    None
}
