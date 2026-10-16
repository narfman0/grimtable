//! Parses the scrubbed fixtures and pins what they contain.

use roster::{
    parse_ros, parse_ros_file, scrub::scrub, Army, Base, CoreRule, UnitEntry, WeaponKind,
};
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(format!("{name}.ros"))
}

fn load(name: &str) -> Army {
    parse_ros_file(fixture(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn units_named<'a>(army: &'a Army, name: &str) -> Vec<&'a UnitEntry> {
    army.units.iter().filter(|u| u.name == name).collect()
}

#[test]
fn mechanicus_header() {
    let army = load("mechanicus");
    assert_eq!(army.faction, "Imperium - Adeptus Mechanicus");
    assert_eq!(army.game_system, "Warhammer 40,000 11th Edition");
    assert_eq!(army.points, 1980);
    assert_eq!(
        army.battle_size.as_deref(),
        Some("Strike Force (2000 Point limit)")
    );
    assert_eq!(army.detachment.as_deref(), Some("Lords of the Forge"));
    assert_eq!(army.force_disposition.as_deref(), Some("Priority Assets"));
    assert_eq!(army.units.len(), 22);
    assert!(army
        .units
        .iter()
        .all(|u| !u.keywords.iter().any(|k| k == "Configuration")));
    assert!(army.units.iter().all(|u| u
        .keywords
        .iter()
        .any(|k| k == "Faction: Adeptus Mechanicus")));
}

#[test]
fn mechanicus_skitarii_rangers() {
    let army = load("mechanicus");
    let rangers = units_named(&army, "Skitarii Rangers");
    assert_eq!(rangers.len(), 2);
    let unit = rangers[0];
    assert_eq!(unit.points, 85);
    assert_eq!(unit.models.len(), 2);
    assert_eq!(unit.models[0].name, "Skitarii Ranger Alpha");
    assert_eq!(unit.models[0].count, 1);
    assert_eq!(unit.models[1].count, 9);
    assert_eq!(unit.models.iter().map(|m| m.count).sum::<u32>(), 10);
    assert_eq!(
        unit.models[1].weapons,
        vec![
            "Close combat weapon".to_string(),
            "Galvanic rifle".to_string()
        ]
    );
    assert!(unit.models.iter().all(|m| m.stats.is_none()));

    let s = &unit.stats;
    assert_eq!(s.movement, 6);
    assert_eq!(s.toughness, 3);
    assert_eq!(s.save, 4);
    assert_eq!(s.wounds, 1);
    assert_eq!(s.leadership, 7);
    assert_eq!(s.objective_control, 2);
    assert_eq!(s.invuln, Some(5));
    assert_eq!(s.base, Base::Round { mm: 25 });

    assert!(unit.rules.iter().any(|r| r == "Scouts 6\""));
    assert!(unit.core_rules.contains(&CoreRule::Scouts(6)));
    assert!(unit
        .core_rules
        .contains(&CoreRule::Other("Doctrina Imperatives".into())));
    assert_eq!(unit.abilities, vec!["Objective Scouted".to_string()]);
    assert!(unit.keywords.iter().any(|k| k == "Battleline"));
    assert!(unit.enhancements.is_empty());
    assert!(unit.can_lead.is_empty());

    let rifle = unit
        .weapons
        .iter()
        .find(|w| w.name == "Galvanic rifle")
        .expect("rifle");
    assert_eq!(rifle.kind, WeaponKind::Ranged);
    assert_eq!(rifle.range, Some(30));
    assert_eq!(rifle.attacks, "2");
    assert_eq!(rifle.skill, Some(4));
    assert_eq!(rifle.strength, 4);
    assert_eq!(rifle.ap, 0);
    assert_eq!(rifle.damage, "1");
    assert!(rifle.keywords.is_empty());
}

#[test]
fn mechanicus_dominus_with_enhancement() {
    let army = load("mechanicus");
    let dominus = units_named(&army, "Tech-Priest Dominus");
    assert_eq!(dominus.len(), 2);
    let with = dominus
        .iter()
        .find(|u| !u.enhancements.is_empty())
        .expect("a Dominus with an enhancement");
    assert_eq!(with.enhancements.len(), 1);
    assert_eq!(with.enhancements[0].name, "TL-409");
    assert_eq!(with.enhancements[0].points, 30);
    assert_eq!(with.points, 90);
    assert_eq!(with.models.len(), 1);
    assert_eq!(with.models[0].count, 1);
    assert!(with.models[0].weapons.iter().any(|w| w == "TL-409"));

    let tl = with
        .weapons
        .iter()
        .find(|w| w.name == "TL-409")
        .expect("TL-409");
    assert_eq!(tl.kind, WeaponKind::Ranged);
    assert_eq!(tl.range, Some(24));
    assert_eq!(tl.skill, Some(2));
    assert_eq!(tl.strength, 11);
    assert_eq!(tl.ap, 2);
    assert_eq!(tl.damage, "D3+2");
    assert_eq!(
        tl.keywords,
        vec!["Devastating Wounds".to_string(), "Hazardous".to_string()]
    );

    assert!(with.core_rules.contains(&CoreRule::Leader));
    assert!(with.core_rules.contains(&CoreRule::FeelNoPain(5)));
    assert!(with
        .can_lead
        .iter()
        .any(|u| u.eq_ignore_ascii_case("Skitarii Rangers")));
    assert!(with
        .can_lead
        .iter()
        .any(|u| u.eq_ignore_ascii_case("Kataphron Breachers")));
    assert_eq!(with.can_lead.len(), 11);
    assert!(with.abilities.iter().any(|a| a == "Leader"));
    assert!(with.transport_capacity.is_none());
}

#[test]
fn mechanicus_dunerider_transport() {
    let army = load("mechanicus");
    let dunerider = units_named(&army, "Skorpius Dunerider")[0];
    assert_eq!(dunerider.transport_capacity, Some(12));
    assert!(dunerider.core_rules.contains(&CoreRule::FiringDeck(2)));
    assert!(dunerider
        .core_rules
        .contains(&CoreRule::DeadlyDemise("D3".into())));
    assert_eq!(dunerider.stats.base, Base::Hull);
    assert_eq!(dunerider.stats.invuln, None);
    let array = dunerider
        .weapons
        .iter()
        .find(|w| w.name == "Cognis heavy stubber array")
        .expect("stubber array");
    assert_eq!(
        array.keywords,
        vec![
            "Rapid Fire 9".to_string(),
            "Sustained Hits 1".to_string(),
            "Twin-linked".to_string()
        ]
    );
}

#[test]
fn mechanicus_odd_shapes() {
    let army = load("mechanicus");

    // Stats live on the model group, not the unit; the unit inherits them.
    let strider = units_named(&army, "Ironstrider Ballistarii")[0];
    assert_eq!(strider.stats.wounds, 7);
    assert_eq!(strider.models[0].stats.as_ref().map(|s| s.wounds), Some(7));
    assert_eq!(
        strider.stats.base,
        Base::Oval {
            mm_long: 105,
            mm_short: 70
        }
    );

    // A model group with its own profile inside a unit that also has one.
    let pteraxii = units_named(&army, "Pteraxii Sterylizors")[0];
    assert_eq!(pteraxii.stats.invuln, Some(5));
    let alpha = &pteraxii.models[0];
    assert_eq!(alpha.stats.as_ref().map(|s| s.invuln), Some(None));
    let torch = pteraxii
        .weapons
        .iter()
        .find(|w| w.name == "Phosphor torch")
        .unwrap();
    assert_eq!(torch.skill, None);
    assert_eq!(torch.attacks, "D6");

    // Two pistols per model.
    let hounds = units_named(&army, "Serberys Sulphurhounds")[0];
    let twin = hounds.models.iter().find(|m| m.count == 2).unwrap();
    assert_eq!(
        twin.weapons
            .iter()
            .filter(|w| *w == "Phosphor pistol")
            .count(),
        2
    );
    assert_eq!(
        hounds.stats.base,
        Base::Oval {
            mm_long: 60,
            mm_short: 36
        }
    );

    // Nested upgrade group, and a two-profile weapon.
    let crawlers = units_named(&army, "Onager Dunecrawler");
    let names: Vec<Vec<&str>> = crawlers
        .iter()
        .map(|c| c.weapons.iter().map(|w| w.name.as_str()).collect())
        .collect();
    assert!(names
        .iter()
        .any(|n| n.contains(&"Neutron laser") && n.contains(&"Cognis heavy stubber")));
    assert!(names.iter().any(|n| n
        .iter()
        .filter(|w| w.contains("Eradication beamer"))
        .count()
        == 2));
    let laser = crawlers
        .iter()
        .flat_map(|c| c.weapons.iter())
        .find(|w| w.name == "Neutron laser")
        .unwrap();
    assert_eq!(laser.damage, "D6+2");
    assert_eq!(laser.ap, 4);

    // Rules on weapon upgrades do not leak into unit rules.
    let breachers = units_named(&army, "Kataphron Breachers")[0];
    assert_eq!(breachers.rules, vec!["Doctrina Imperatives".to_string()]);
    let rifle = breachers
        .weapons
        .iter()
        .find(|w| w.name == "Heavy arc rifle")
        .unwrap();
    assert_eq!(
        rifle.keywords,
        vec![
            "Anti-Vehicle 4+".to_string(),
            "Rapid Fire 2".to_string(),
            "Lethal Hits".to_string()
        ]
    );
}

#[test]
fn custodes() {
    let army = load("custodes");
    assert_eq!(army.points, 1830);
    assert_eq!(army.faction, "Imperium - Adeptus Custodes");
    assert_eq!(army.detachment.as_deref(), Some("Shield Host"));
    assert_eq!(army.units.len(), 12);
    println!("custodes: {} units", army.units.len());

    // One upgrade with a ranged and a melee profile of the same name.
    let guard = units_named(&army, "Custodian Guard")[0];
    let spears: Vec<_> = guard
        .weapons
        .iter()
        .filter(|w| w.name == "Guardian Spear")
        .collect();
    assert_eq!(spears.len(), 2);
    assert!(spears
        .iter()
        .any(|w| w.kind == WeaponKind::Ranged && w.range == Some(24)));
    assert!(spears
        .iter()
        .any(|w| w.kind == WeaponKind::Melee && w.range.is_none()));
    let four = guard.models.iter().find(|m| m.count == 4).unwrap();
    assert_eq!(four.weapons, vec!["Guardian Spear".to_string()]);
    // Vexilla bearer has its own profile with more wounds.
    let vex = guard.models.iter().find(|m| m.count == 1).unwrap();
    assert_eq!(vex.stats.as_ref().map(|s| s.wounds), Some(4));
    assert_eq!(guard.stats.wounds, 3);
    assert!(guard.abilities.iter().any(|a| a == "Vexilla"));

    let prosecutors = units_named(&army, "Prosecutors")[0];
    assert!(prosecutors.core_rules.contains(&CoreRule::FeelNoPain(3)));
    assert_eq!(prosecutors.rules, vec!["Feel No Pain 3+*".to_string()]);

    let raider = units_named(&army, "Venerable Land Raider")[0];
    assert_eq!(raider.transport_capacity, Some(6));
    assert_eq!(
        raider.models[0]
            .weapons
            .iter()
            .filter(|w| *w == "Godhammer lascannon")
            .count(),
        2
    );
    assert!(raider
        .core_rules
        .contains(&CoreRule::DeadlyDemise("D6".into())));

    let trajann = units_named(&army, "Trajann Valoris")[0];
    assert!(trajann.core_rules.contains(&CoreRule::DeepStrike));
    assert_eq!(trajann.can_lead.len(), 4);
    assert!(trajann
        .can_lead
        .iter()
        .any(|u| u.eq_ignore_ascii_case("Custodian Guard")));

    let champion = units_named(&army, "Blade Champion")[0];
    assert_eq!(champion.enhancements.len(), 1);
    assert_eq!(champion.enhancements[0].name, "Auric Mantle");
    assert_eq!(champion.points, 125);
}

#[test]
fn necrons() {
    let army = load("necrons");
    assert_eq!(army.points, 2055);
    assert_eq!(army.faction, "Xenos - Necrons");
    assert_eq!(army.detachment.as_deref(), Some("Awakened Dynasty"));
    assert_eq!(army.units.len(), 22);
    println!("necrons: {} units", army.units.len());

    let warriors = units_named(&army, "Necron Warriors")[0];
    assert_eq!(warriors.models.iter().map(|m| m.count).sum::<u32>(), 10);
    assert_eq!(warriors.stats.invuln, Some(4));

    let flayed = units_named(&army, "Flayed Ones")[0];
    assert_eq!(flayed.stats.base, Base::Round { mm: 29 });
    assert!(flayed.core_rules.contains(&CoreRule::Infiltrators));
    assert!(flayed.core_rules.contains(&CoreRule::Stealth));

    let trazyn = units_named(&army, "Trazyn the Infinite")[0];
    let obliterator = &trazyn.weapons[0];
    assert_eq!(obliterator.keywords, vec!["Sustained Hits D3".to_string()]);
    assert_eq!(obliterator.damage, "D3");
    assert_eq!(trazyn.can_lead.len(), 3);

    let hexmark = units_named(&army, "Hexmark Destroyer")[0];
    assert!(hexmark.core_rules.contains(&CoreRule::LoneOperative));

    let ctan = units_named(&army, "C'tan Shard of the Void Dragon")[0];
    assert_eq!(ctan.points, 345);
    assert_eq!(ctan.weapons.len(), 5);
    let spear = ctan
        .weapons
        .iter()
        .find(|w| w.name == "Spear of the Void Dragon")
        .unwrap();
    assert_eq!(spear.keywords, vec!["Anti-VEHICLE 2+".to_string()]);
    assert_eq!(spear.attacks, "D3");

    let orikan = units_named(&army, "Orikan the Diviner")[0];
    assert!(orikan.core_rules.contains(&CoreRule::Support));
}

#[test]
fn serde_round_trip() {
    let army = load("necrons");
    let json = serde_json::to_string(&army).unwrap();
    let back: Army = serde_json::from_str(&json).unwrap();
    assert_eq!(army, back);
}

#[test]
fn scrub_is_idempotent_and_prose_free() {
    for name in ["mechanicus", "custodes", "necrons"] {
        let xml = std::fs::read_to_string(fixture(name)).unwrap();
        assert_eq!(
            scrub(&xml).unwrap(),
            xml,
            "{name}: fixture is already scrubbed"
        );
        for phrase in [
            "Each time",
            "While this",
            "At the start",
            "this unit",
            "this model",
        ] {
            assert!(!xml.contains(phrase), "{name}: fixture contains {phrase:?}");
        }
    }
}

/// The scrubbed fixtures parse to the same army as the unscrubbed exports.
/// Skipped when the exports are not on this machine.
#[test]
fn scrubbed_matches_original() {
    let lists = Path::new("/home/narfman0/workspace/40k/lists");
    if !lists.is_dir() {
        eprintln!("skipping: {} not found", lists.display());
        return;
    }
    for name in ["mechanicus", "custodes", "necrons"] {
        let original = std::fs::read_to_string(lists.join(format!("2026-10-15-{name}.ros")))
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let scrubbed = scrub(&original).unwrap();
        assert_eq!(
            parse_ros(&original).unwrap(),
            parse_ros(&scrubbed).unwrap(),
            "{name}"
        );
        assert_eq!(
            scrubbed,
            std::fs::read_to_string(fixture(name)).unwrap(),
            "{name}: fixture is stale"
        );
    }
}
