use engine::attack::WeaponAbility;
use engine::dice::DiceExpr;
use roster::parse_weapon_keyword;

#[test]
fn plain_keywords() {
    let cases = [
        ("Assault", WeaponAbility::Assault),
        ("Blast", WeaponAbility::Blast),
        ("Devastating Wounds", WeaponAbility::DevastatingWounds),
        ("Extra Attacks", WeaponAbility::ExtraAttacks),
        ("Hazardous", WeaponAbility::Hazardous),
        ("Heavy", WeaponAbility::Heavy),
        ("Ignores Cover", WeaponAbility::IgnoresCover),
        ("Indirect Fire", WeaponAbility::IndirectFire),
        ("Lance", WeaponAbility::Lance),
        ("Lethal Hits", WeaponAbility::LethalHits),
        ("Pistol", WeaponAbility::Pistol),
        ("Precision", WeaponAbility::Precision),
        ("Psychic", WeaponAbility::Psychic),
        ("Torrent", WeaponAbility::Torrent),
        ("Twin-linked", WeaponAbility::TwinLinked),
    ];
    for (text, want) in cases {
        assert_eq!(parse_weapon_keyword(text), Some(want.clone()), "{text}");
        assert_eq!(
            parse_weapon_keyword(&text.to_uppercase()),
            Some(want.clone()),
            "{text} upper"
        );
        assert_eq!(
            parse_weapon_keyword(&format!("  {text}\u{a0}")),
            Some(want),
            "{text} padded"
        );
    }
}

#[test]
fn parameterised_keywords() {
    assert_eq!(
        parse_weapon_keyword("Melta 3"),
        Some(WeaponAbility::Melta(3))
    );
    assert_eq!(
        parse_weapon_keyword("Rapid Fire 9"),
        Some(WeaponAbility::RapidFire(9))
    );
    assert_eq!(
        parse_weapon_keyword("Sustained Hits 1"),
        Some(WeaponAbility::SustainedHits(DiceExpr::flat(1)))
    );
    assert_eq!(
        parse_weapon_keyword("Sustained Hits D3"),
        Some(WeaponAbility::SustainedHits(DiceExpr::d3(1)))
    );
    assert_eq!(
        parse_weapon_keyword("Anti-Vehicle 4+"),
        Some(WeaponAbility::Anti {
            keyword: "Vehicle".into(),
            threshold: 4
        })
    );
    assert_eq!(
        parse_weapon_keyword("Anti-VEHICLE 2+"),
        Some(WeaponAbility::Anti {
            keyword: "VEHICLE".into(),
            threshold: 2
        })
    );
    assert_eq!(
        parse_weapon_keyword("anti-infantry 3+"),
        Some(WeaponAbility::Anti {
            keyword: "infantry".into(),
            threshold: 3
        })
    );
}

#[test]
fn unknown_keywords() {
    assert_eq!(parse_weapon_keyword(""), None);
    assert_eq!(parse_weapon_keyword("Melta"), None);
    assert_eq!(parse_weapon_keyword("Rapid Fire"), None);
    assert_eq!(parse_weapon_keyword("Anti-Vehicle"), None);
    assert_eq!(parse_weapon_keyword("One Shot"), None);
}
