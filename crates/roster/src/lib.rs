//! Parses New Recruit / BattleScribe `.ros` army lists into typed data.
//!
//! Only numbers, names, keywords and ids are extracted. Ability and rule
//! descriptions are never read into the output; the [`scrub`] module exists
//! to strip them out of files before they are committed as fixtures.
//!
//! Conventions worth knowing:
//! - `entry_id` is the last `::`-separated segment of the selection's
//!   `entryId`, which is the catalogue entry id without its import prefix.
//! - A unit's `points` is the sum of every `pts` cost in its subtree, so it
//!   includes enhancements. `Army::points` is the sum over units.
//! - `can_lead` keeps the unit names exactly as the file prints them (upper
//!   case). Compare case-insensitively.
//! - `weapons` is deduplicated by `(name, kind)`: a weapon with a ranged and
//!   a melee profile of the same name yields two entries.
//! - `rules` come from the unit and its model selections only; rules that
//!   sit on weapon upgrades duplicate the weapon's keywords and are skipped.
//! - Base sizes with a fractional millimetre value are rounded to the
//!   nearest whole millimetre.

pub mod keywords;
pub mod scrub;

pub use keywords::parse_weapon_keyword;

use roxmltree::{Document, Node};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::Path;

/// The XML namespace every `.ros` file uses.
pub const ROSTER_NS: &str = "http://www.battlescribe.net/schema/rosterSchema";

#[derive(Debug)]
pub enum RosterError {
    Xml(roxmltree::Error),
    Io(std::io::Error),
    /// The roster has no `<force>`.
    NoForce,
    /// A unit has no `Unit` profile on itself or any model group.
    MissingStats(String),
    /// A characteristic could not be parsed. `context` names the unit or
    /// profile it came from.
    Invalid {
        what: &'static str,
        value: String,
        context: String,
    },
}

impl fmt::Display for RosterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RosterError::Xml(e) => write!(f, "xml: {e}"),
            RosterError::Io(e) => write!(f, "io: {e}"),
            RosterError::NoForce => write!(f, "roster has no force"),
            RosterError::MissingStats(unit) => write!(f, "{unit}: no Unit profile found"),
            RosterError::Invalid {
                what,
                value,
                context,
            } => write!(f, "{context}: cannot parse {what} from {value:?}"),
        }
    }
}

impl std::error::Error for RosterError {}

impl From<roxmltree::Error> for RosterError {
    fn from(e: roxmltree::Error) -> Self {
        RosterError::Xml(e)
    }
}

impl From<std::io::Error> for RosterError {
    fn from(e: std::io::Error) -> Self {
        RosterError::Io(e)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Army {
    pub name: String,
    pub game_system: String,
    /// The force's `catalogueName`, e.g. "Imperium - Adeptus Mechanicus".
    pub faction: String,
    pub detachment: Option<String>,
    pub battle_size: Option<String>,
    pub force_disposition: Option<String>,
    /// Sum of every unit's points.
    pub points: u32,
    pub units: Vec<UnitEntry>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnitEntry {
    pub name: String,
    pub entry_id: String,
    /// Includes enhancements.
    pub points: u32,
    /// Every category name except `Configuration`, including `Faction: X`.
    pub keywords: Vec<String>,
    /// The unit's own profile, or the first model group's if the unit has none.
    pub stats: Stats,
    pub models: Vec<ModelGroup>,
    /// Deduplicated by `(name, kind)`.
    pub weapons: Vec<WeaponProfile>,
    /// Ability names only, from the unit, its models and its wargear.
    pub abilities: Vec<String>,
    /// Raw rule names from the unit and its model selections.
    pub rules: Vec<String>,
    /// `rules`, parsed.
    pub core_rules: Vec<CoreRule>,
    pub enhancements: Vec<Enhancement>,
    /// Unit names this leader may attach to, as printed (upper case).
    pub can_lead: Vec<String>,
    pub transport_capacity: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stats {
    /// Inches.
    pub movement: u32,
    pub toughness: u32,
    /// The number needed, e.g. 3 for 3+.
    pub save: u8,
    pub wounds: u32,
    pub leadership: u8,
    pub objective_control: u32,
    pub invuln: Option<u8>,
    pub base: Base,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Base {
    Round { mm: u32 },
    Oval { mm_long: u32, mm_short: u32 },
    Hull,
    Unknown(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelGroup {
    pub name: String,
    pub count: u32,
    /// Set when this group carries its own `Unit` profile.
    pub stats: Option<Stats>,
    /// Weapon profile names, one entry per weapon carried by each model. A
    /// weapon whose upgrade has several profiles lists every profile name.
    pub weapons: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WeaponKind {
    Ranged,
    Melee,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WeaponProfile {
    pub name: String,
    pub kind: WeaponKind,
    /// Inches; `None` for melee.
    pub range: Option<u32>,
    /// Kept as a string because it may be a dice expression.
    pub attacks: String,
    /// BS or WS as the number needed; `None` for "N/A".
    pub skill: Option<u8>,
    pub strength: u32,
    /// Magnitude: "-2" is 2.
    pub ap: u32,
    pub damage: String,
    /// Raw keyword text, trimmed, e.g. "Rapid Fire 9". See
    /// [`parse_weapon_keyword`] to map them onto the engine's enum.
    pub keywords: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Enhancement {
    pub name: String,
    pub points: u32,
}

/// Core rules the edition defines by name. Anything else is `Other`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CoreRule {
    /// The dice expression, e.g. "1", "D3", "D6".
    DeadlyDemise(String),
    DeepStrike,
    /// The number needed, e.g. 5 for 5+.
    FeelNoPain(u8),
    FiringDeck(u32),
    Infiltrators,
    Leader,
    LoneOperative,
    /// Inches.
    Scouts(u32),
    Stealth,
    Support,
    FightsFirst,
    Other(String),
}

impl CoreRule {
    /// Parses a rule name such as `Feel No Pain 5+`, `Scouts 6"` or
    /// `Deadly Demise D3`. A trailing `*` is ignored.
    pub fn parse(name: &str) -> CoreRule {
        let s = normalize(name);
        let s = s.trim_end_matches('*').trim();
        match s.to_ascii_lowercase().as_str() {
            "deep strike" => return CoreRule::DeepStrike,
            "infiltrators" => return CoreRule::Infiltrators,
            "leader" => return CoreRule::Leader,
            "lone operative" => return CoreRule::LoneOperative,
            "stealth" => return CoreRule::Stealth,
            "support" => return CoreRule::Support,
            "fights first" => return CoreRule::FightsFirst,
            _ => {}
        }
        if let Some(rest) = strip_prefix_ci(s, "deadly demise ") {
            return CoreRule::DeadlyDemise(rest.trim().to_ascii_uppercase());
        }
        if let Some(n) = strip_prefix_ci(s, "feel no pain ").and_then(parse_plus) {
            return CoreRule::FeelNoPain(n);
        }
        if let Some(n) = strip_prefix_ci(s, "firing deck ").and_then(|r| r.trim().parse().ok()) {
            return CoreRule::FiringDeck(n);
        }
        if let Some(n) = strip_prefix_ci(s, "scouts ").and_then(parse_inches) {
            return CoreRule::Scouts(n);
        }
        CoreRule::Other(name.trim().to_string())
    }
}

/// Parses a `.ros` document.
pub fn parse_ros(xml: &str) -> Result<Army, RosterError> {
    let doc = Document::parse(xml)?;
    let roster = doc.root_element();
    let forces: Vec<Node> = children(roster, "forces")
        .flat_map(|f| children(f, "force"))
        .collect();
    let first = forces.first().copied().ok_or(RosterError::NoForce)?;

    let mut army = Army {
        name: attr(roster, "name"),
        game_system: attr(roster, "gameSystemName"),
        faction: attr(first, "catalogueName"),
        detachment: None,
        battle_size: None,
        force_disposition: None,
        points: 0,
        units: Vec::new(),
    };

    for force in forces {
        for sel in selections(force) {
            if is_configuration(sel) {
                let child = selections(sel).next().map(|c| attr(c, "name"));
                match attr(sel, "name").as_str() {
                    "Battle Size" => army.battle_size = child,
                    "Detachment" => army.detachment = child,
                    "Force Disposition" => army.force_disposition = child,
                    _ => {}
                }
            } else {
                army.units.push(parse_unit(sel)?);
            }
        }
    }
    army.points = army.units.iter().map(|u| u.points).sum();
    Ok(army)
}

/// Reads and parses a `.ros` file.
pub fn parse_ros_file(path: impl AsRef<Path>) -> Result<Army, RosterError> {
    let xml = std::fs::read_to_string(path)?;
    parse_ros(&xml)
}

// ---------------------------------------------------------------------------
// Units

#[derive(Default)]
struct Collected {
    weapons: Vec<WeaponProfile>,
    abilities: Vec<String>,
    rules: Vec<String>,
    enhancements: Vec<Enhancement>,
    can_lead: Vec<String>,
    transport_capacity: Option<u32>,
}

fn parse_unit(sel: Node) -> Result<UnitEntry, RosterError> {
    let name = attr(sel, "name");
    let kind = attr(sel, "type");
    let mut acc = Collected::default();
    collect(sel, &mut acc, &name)?;

    let models = if kind == "unit" {
        selections(sel)
            .filter(|m| m.attribute("type") == Some("model"))
            .map(|m| parse_model_group(m, &name))
            .collect::<Result<Vec<_>, _>>()?
    } else {
        vec![ModelGroup {
            name: name.clone(),
            count: number(sel),
            stats: None,
            weapons: carried_weapons(sel, number(sel)),
        }]
    };

    let stats = match unit_profile(sel) {
        Some(p) => parse_stats(p, &name)?,
        None => models
            .iter()
            .find_map(|m| m.stats.clone())
            .ok_or_else(|| RosterError::MissingStats(name.clone()))?,
    };

    let core_rules = acc.rules.iter().map(|r| CoreRule::parse(r)).collect();
    Ok(UnitEntry {
        entry_id: entry_id(sel),
        points: subtree_points(sel),
        keywords: categories(sel)
            .map(|c| attr(c, "name"))
            .filter(|c| c != "Configuration")
            .collect(),
        stats,
        models,
        weapons: acc.weapons,
        abilities: acc.abilities,
        rules: acc.rules,
        core_rules,
        enhancements: acc.enhancements,
        can_lead: acc.can_lead,
        transport_capacity: acc.transport_capacity,
        name,
    })
}

fn parse_model_group(model: Node, unit: &str) -> Result<ModelGroup, RosterError> {
    let name = attr(model, "name");
    let count = number(model);
    let stats = match unit_profile(model) {
        Some(p) => Some(parse_stats(p, &format!("{unit} / {name}"))?),
        None => None,
    };
    Ok(ModelGroup {
        weapons: carried_weapons(model, count),
        name,
        count,
        stats,
    })
}

/// Weapon profile names carried by each model of a group of `count`, from
/// every upgrade below `node`. An upgrade numbered `2 * count` is two per
/// model.
fn carried_weapons(node: Node, count: u32) -> Vec<String> {
    let mut out = Vec::new();
    for up in selections(node) {
        if up.attribute("type") == Some("model") {
            continue;
        }
        let mut names: Vec<String> = Vec::new();
        for p in profiles(up).filter(|p| weapon_kind(*p).is_some()) {
            push_unique(&mut names, attr(p, "name"));
        }
        let per_model = (number(up) / count.max(1)).max(1);
        for _ in 0..per_model {
            out.extend(names.iter().cloned());
        }
        out.extend(carried_weapons(up, count));
    }
    out
}

/// Walks a selection and everything below it, gathering profiles, rules and
/// enhancements.
fn collect(node: Node, acc: &mut Collected, unit: &str) -> Result<(), RosterError> {
    let kind = node.attribute("type").unwrap_or("");
    for p in profiles(node) {
        match p.attribute("typeName") {
            Some("Abilities") => {
                let name = attr(p, "name");
                if name == "Leader" || name == "Support" {
                    if let Some(desc) = characteristic(p, "Description") {
                        acc.can_lead = parse_leader_list(desc);
                    }
                }
                push_unique(&mut acc.abilities, name);
            }
            Some("Transport") => {
                if let Some(desc) = characteristic(p, "Capacity") {
                    acc.transport_capacity = parse_capacity(desc);
                }
            }
            Some("Ranged Weapons") | Some("Melee Weapons") => {
                let w = parse_weapon(p, unit)?;
                if !acc
                    .weapons
                    .iter()
                    .any(|x| x.name == w.name && x.kind == w.kind)
                {
                    acc.weapons.push(w);
                }
            }
            _ => {}
        }
    }
    if kind == "upgrade" {
        let pts = own_points(node);
        if pts > 0 {
            acc.enhancements.push(Enhancement {
                name: attr(node, "name"),
                points: pts,
            });
        }
    } else {
        for r in rules(node) {
            push_unique(&mut acc.rules, attr(r, "name"));
        }
    }
    for child in selections(node) {
        collect(child, acc, unit)?;
    }
    Ok(())
}

fn parse_stats(p: Node, context: &str) -> Result<Stats, RosterError> {
    let get = |name: &'static str| -> Result<String, RosterError> {
        characteristic(p, name)
            .map(normalize)
            .ok_or_else(|| RosterError::Invalid {
                what: name,
                value: String::new(),
                context: context.to_string(),
            })
    };
    let invalid = |what: &'static str, value: &str| RosterError::Invalid {
        what,
        value: value.to_string(),
        context: context.to_string(),
    };
    let m = get("M")?;
    let t = get("T")?;
    let sv = get("Sv")?;
    let w = get("W")?;
    let ld = get("LD")?;
    let oc = get("OC")?;
    let insv = characteristic(p, "InSv").map(normalize).unwrap_or_default();
    let base = get("Base")?;
    Ok(Stats {
        movement: parse_inches(&m).ok_or_else(|| invalid("M", &m))?,
        toughness: t.parse().map_err(|_| invalid("T", &t))?,
        save: parse_plus(&sv).ok_or_else(|| invalid("Sv", &sv))?,
        wounds: w.parse().map_err(|_| invalid("W", &w))?,
        leadership: parse_plus(&ld).ok_or_else(|| invalid("LD", &ld))?,
        objective_control: oc.parse().map_err(|_| invalid("OC", &oc))?,
        invuln: if insv.is_empty() || insv == "-" {
            None
        } else {
            Some(parse_plus(&insv).ok_or_else(|| invalid("InSv", &insv))?)
        },
        base: parse_base(&base),
    })
}

/// `32mm`, `60x35mm`, `Hull`. Fractional values are rounded.
pub fn parse_base(s: &str) -> Base {
    let s = normalize(s);
    if s.eq_ignore_ascii_case("hull") {
        return Base::Hull;
    }
    let Some(body) = s.strip_suffix("mm") else {
        return Base::Unknown(s);
    };
    let mm = |v: &str| v.trim().parse::<f64>().ok().map(|f| f.round() as u32);
    match body.split_once('x') {
        Some((a, b)) => match (mm(a), mm(b)) {
            (Some(a), Some(b)) => Base::Oval {
                mm_long: a.max(b),
                mm_short: a.min(b),
            },
            _ => Base::Unknown(s),
        },
        None => match mm(body) {
            Some(mm) => Base::Round { mm },
            None => Base::Unknown(s),
        },
    }
}

fn parse_weapon(p: Node, unit: &str) -> Result<WeaponProfile, RosterError> {
    let name = attr(p, "name");
    let kind = weapon_kind(p).expect("caller checked typeName");
    let context = format!("{unit} / {name}");
    let get = |c: &str| characteristic(p, c).map(normalize).unwrap_or_default();
    let invalid = |what: &'static str, value: &str| RosterError::Invalid {
        what,
        value: value.to_string(),
        context: context.clone(),
    };
    let range = get("Range");
    let range = if range.eq_ignore_ascii_case("melee") || range.is_empty() {
        None
    } else {
        Some(parse_inches(&range).ok_or_else(|| invalid("Range", &range))?)
    };
    let skill = match kind {
        WeaponKind::Ranged => get("BS"),
        WeaponKind::Melee => get("WS"),
    };
    let skill = if skill.eq_ignore_ascii_case("n/a") || skill.is_empty() || skill == "-" {
        None
    } else {
        Some(parse_plus(&skill).ok_or_else(|| invalid("skill", &skill))?)
    };
    let s = get("S");
    let ap = get("AP");
    Ok(WeaponProfile {
        range,
        attacks: get("A"),
        skill,
        strength: s.parse().map_err(|_| invalid("S", &s))?,
        ap: ap
            .parse::<i32>()
            .map(|v| v.unsigned_abs())
            .map_err(|_| invalid("AP", &ap))?,
        damage: get("D"),
        keywords: get("Keywords")
            .split(',')
            .map(normalize)
            .filter(|k| !k.is_empty())
            .collect(),
        name,
        kind,
    })
}

/// The unit names after "following units:", or the whole text when that
/// phrase is absent (which is what the scrubber leaves behind).
pub fn parse_leader_list(desc: &str) -> Vec<String> {
    let text = normalize(desc);
    let lower = text.to_ascii_lowercase();
    let list = match lower.find("following units:").map(|i| i + "following units:".len()).or_else(|| lower.find("following unit:").map(|i| i + "following unit:".len())) {
        Some(i) => &text[i..],
        None => text.as_str(),
    };
    list.split(',')
        .map(|s| normalize(s.trim_end_matches('.')))
        .filter(|s| !s.is_empty())
        .collect()
}

/// The number after "capacity of", or the leading number when that phrase
/// is absent (which is what the scrubber leaves behind).
pub fn parse_capacity(desc: &str) -> Option<u32> {
    let text = normalize(desc);
    let marker = "capacity of";
    let rest = match text.to_ascii_lowercase().find(marker) {
        Some(i) => &text[i + marker.len()..],
        None => text.as_str(),
    };
    let digits: String = rest
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

// ---------------------------------------------------------------------------
// XML helpers

fn children<'a, 'i>(n: Node<'a, 'i>, name: &'a str) -> impl Iterator<Item = Node<'a, 'i>> + 'a {
    n.children()
        .filter(move |c| c.is_element() && c.tag_name().name() == name)
}

fn selections<'a, 'i>(n: Node<'a, 'i>) -> impl Iterator<Item = Node<'a, 'i>> + 'a {
    children(n, "selections").flat_map(|s| children(s, "selection"))
}

fn profiles<'a, 'i>(n: Node<'a, 'i>) -> impl Iterator<Item = Node<'a, 'i>> + 'a {
    children(n, "profiles").flat_map(|s| children(s, "profile"))
}

fn categories<'a, 'i>(n: Node<'a, 'i>) -> impl Iterator<Item = Node<'a, 'i>> + 'a {
    children(n, "categories").flat_map(|s| children(s, "category"))
}

fn rules<'a, 'i>(n: Node<'a, 'i>) -> impl Iterator<Item = Node<'a, 'i>> + 'a {
    children(n, "rules").flat_map(|s| children(s, "rule"))
}

fn attr(n: Node, name: &str) -> String {
    n.attribute(name).unwrap_or("").to_string()
}

fn number(n: Node) -> u32 {
    n.attribute("number")
        .and_then(|v| v.parse().ok())
        .unwrap_or(1)
}

fn entry_id(n: Node) -> String {
    let id = n.attribute("entryId").unwrap_or("");
    id.rsplit("::").next().unwrap_or(id).to_string()
}

fn is_configuration(sel: Node) -> bool {
    categories(sel).any(|c| c.attribute("name") == Some("Configuration"))
}

fn unit_profile<'a, 'i>(n: Node<'a, 'i>) -> Option<Node<'a, 'i>> {
    profiles(n).find(|p| p.attribute("typeName") == Some("Unit"))
}

fn weapon_kind(p: Node) -> Option<WeaponKind> {
    match p.attribute("typeName") {
        Some("Ranged Weapons") => Some(WeaponKind::Ranged),
        Some("Melee Weapons") => Some(WeaponKind::Melee),
        _ => None,
    }
}

fn characteristic<'a>(profile: Node<'a, '_>, name: &str) -> Option<&'a str> {
    children(profile, "characteristics")
        .flat_map(|c| children(c, "characteristic"))
        .find(|c| c.attribute("name") == Some(name))
        .map(|c| c.text().unwrap_or(""))
}

fn own_points(n: Node) -> u32 {
    children(n, "costs")
        .flat_map(|c| children(c, "cost"))
        .filter(|c| c.attribute("name") == Some("pts"))
        .filter_map(|c| c.attribute("value")?.parse::<f64>().ok())
        .map(|v| v.round() as u32)
        .sum()
}

fn subtree_points(n: Node) -> u32 {
    own_points(n) + selections(n).map(subtree_points).sum::<u32>()
}

fn push_unique(v: &mut Vec<String>, s: String) {
    if !v.contains(&s) {
        v.push(s);
    }
}

// ---------------------------------------------------------------------------
// Value helpers

/// Trims, and collapses non-breaking spaces to plain spaces.
pub(crate) fn normalize(s: &str) -> String {
    s.replace('\u{a0}', " ").trim().to_string()
}

pub(crate) fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let head = s.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &s[prefix.len()..])
}

/// "4+" → 4.
pub(crate) fn parse_plus(s: &str) -> Option<u8> {
    s.trim().trim_end_matches('+').trim().parse().ok()
}

/// `6"` → 6. Also accepts a bare number.
pub(crate) fn parse_inches(s: &str) -> Option<u32> {
    s.trim()
        .trim_end_matches(['"', '\u{201d}', '\u{2033}'])
        .trim()
        .parse()
        .ok()
}
