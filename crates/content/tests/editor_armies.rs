//! An editor-built army mirroring the user's Mechanicus list: validates,
//! builds a pack, and plays.

use content::army_def::{self, ArmyDef, ModelRowDef, UnitDef};
use content::catalogue;
use engine::state::GameState;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

fn cat(slug: &str) -> catalogue::Catalogue {
    let root = root();
    let bs = catalogue::bsdata_file(slug).and_then(|f| catalogue::bsdata_points(&root, f).ok());
    catalogue::build(&root, slug, bs.as_ref()).unwrap()
}

fn unit(c: &catalogue::Catalogue, id: &str) -> UnitDef {
    let sheet = c.units.iter().find(|u| u.id == id).unwrap_or_else(|| panic!("{id}"));
    army_def::default_unit(sheet)
}

fn with_count(mut u: UnitDef, row: &str, count: u32) -> UnitDef {
    for m in &mut u.models {
        if m.row == row {
            m.count = count;
        }
    }
    u
}

#[test]
fn catalogue_has_the_mechanicus_essentials() {
    let c = cat("adeptus-mechanicus");
    let rangers = c.units.iter().find(|u| u.id == "skitarii-rangers").unwrap();
    assert_eq!(rangers.points.first().map(|p| (p.models, p.cost)), Some((10, 85)));
    assert_eq!((rangers.stats.toughness, rangers.stats.save, rangers.stats.invuln), (3, 4, Some(5)));
    assert_eq!(rangers.models.len(), 2);
    assert!(rangers.is_battleline);
    let dominus = c.units.iter().find(|u| u.id == "tech-priest-dominus").unwrap();
    assert!(dominus.can_lead_ids.contains(&"skitarii-rangers".to_string()));
    let lotf = c.detachments.iter().find(|d| d.id == "lords-of-the-forge").unwrap();
    assert_eq!(lotf.detachment_points, 2, "BSData detachment points overlay");
    let breachers = c.units.iter().find(|u| u.id == "kataphron-breachers").unwrap();
    assert_eq!(breachers.points.first().map(|p| p.cost), Some(140), "BSData base cost overlay");
    let rifle = c.weapons.iter().find(|w| w.id == "galvanic-rifle").unwrap();
    assert_eq!((rifle.kind.as_str(), rifle.profiles[0].range, rifle.profiles[0].skill), ("ranged", Some(30), Some(4)));
    let arquebus = c.weapons.iter().find(|w| w.id == "skatros-transuranic-arquebus").unwrap();
    assert!(arquebus.profiles[0].keywords.iter().any(|k| k == "Anti-Vehicle 4+"), "{:?}", arquebus.profiles[0].keywords);
}

#[test]
fn an_editor_army_validates_and_plays() {
    let c = cat("adeptus-mechanicus");
    let mut cawl = unit(&c, "belisarius-cawl");
    cawl.warlord = true;
    let mut dominus = unit(&c, "tech-priest-dominus");
    dominus.enhancement_id = Some("tl-4-9-lords-of-the-forge".into());
    let def = ArmyDef {
        name: "Editor Mechanicus".into(),
        faction_id: "adeptus-mechanicus".into(),
        detachment_ids: vec!["lords-of-the-forge".into(), "cohort-cybernetica".into()],
        units: vec![
            cawl,
            dominus,
            unit(&c, "tech-priest-dominus"),
            unit(&c, "skitarii-marshal"),
            unit(&c, "tech-priest-manipulus"),
            unit(&c, "cybernetica-datasmith"),
            unit(&c, "sydonian-skatros"),
            unit(&c, "skitarii-rangers"),
            unit(&c, "skitarii-rangers"),
            unit(&c, "skitarii-vanguard"),
            unit(&c, "skitarii-vanguard"),
            unit(&c, "kataphron-breachers"),
            unit(&c, "kastelan-robots"),
            unit(&c, "ironstrider-ballistarii"),
            unit(&c, "onager-dunecrawler"),
            unit(&c, "onager-dunecrawler"),
            unit(&c, "pteraxii-sterylizors"),
            unit(&c, "serberys-raiders"),
            unit(&c, "serberys-sulphurhounds"),
            unit(&c, "sicarian-infiltrators"),
            unit(&c, "sicarian-ruststalkers"),
            unit(&c, "skorpius-dunerider"),
        ],
    };
    let problems = army_def::validate(&def, &c, 2000, 3);
    assert!(problems.is_empty(), "{problems:?}");
    let points = army_def::total_points(&def, &c);
    assert!((1900..=2000).contains(&points), "points {points}");

    // Over the limit and no warlord are both caught.
    let mut bad = def.clone();
    bad.units[0].warlord = false;
    bad.units.push(unit(&c, "belisarius-cawl"));
    bad.units.push(with_count(unit(&c, "kataphron-breachers"), "Kataphron Breacher", 6));
    let problems = army_def::validate(&bad, &c, 2000, 3);
    assert!(problems.iter().any(|p| p.contains("Warlord")), "{problems:?}");
    assert!(problems.iter().any(|p| p.contains("Epic Hero") || p.contains("appears 2 times")), "{problems:?}");

    let abilities = content::enrich::load_abilities(&root(), "adeptus-mechanicus").unwrap();
    let pack = army_def::to_pack(&def, &c, &abilities);
    assert_eq!(pack.units.len(), 22);
    assert!(pack.army_rules.iter().any(|r| r.name == "Doctrina Imperatives"));
    assert!(pack.army_rules.iter().any(|r| r.name == "War-form Mantles"));
    let rangers = pack.units.iter().find(|u| u.name == "Skitarii Rangers").unwrap();
    assert_eq!(rangers.models.iter().map(|m| m.count).sum::<u32>(), 10);
    assert!(rangers.core_rules.contains(&engine::content::CoreRule::Scouts(6)), "{:?}", rangers.core_rules);
    assert!(rangers.weapons.iter().any(|w| w.profile.name == "Galvanic rifle"));
    let dominus = pack.units.iter().find(|u| u.name == "Tech-Priest Dominus").unwrap();
    assert!(dominus.can_lead.iter().any(|n| n == "Skitarii Rangers"));
    assert!(dominus.abilities.iter().any(|a| a.name.starts_with("TL-4")), "{:?}", dominus.abilities.iter().map(|a| &a.name).collect::<Vec<_>>());

    // It plays against the roster-built Necron fixture.
    let necrons = {
        let army = roster::parse_ros_file(root().join("crates/roster/fixtures/necrons.ros")).unwrap();
        let ab = content::enrich::load_abilities(&root(), "necrons").unwrap();
        content::pack::build_pack(&army, &ab)
    };
    let mission = content::mission::default_mission(&root(), "dawn-of-war").unwrap();
    let s = GameState::new([pack, necrons], mission, engine::content::EditionParams::default(), 3);
    assert_eq!(s.units.iter().filter(|u| u.owner == 0).count(), 22);
}
