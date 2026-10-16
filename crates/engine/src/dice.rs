//! Seeded dice. Every random number in a game comes through here so that a
//! game is fully described by its seed and its action log.

use serde::{Deserialize, Serialize};
use std::fmt;

/// A source of D6 results. The real game uses [`Rng`]; tests use [`Scripted`].
pub trait Dice {
    fn d6(&mut self) -> u8;

    fn d3(&mut self) -> u8 {
        (self.d6() + 1) / 2
    }

    fn roll(&mut self, expr: &DiceExpr) -> u32 {
        let mut total = expr.add;
        for _ in 0..expr.count {
            total += match expr.sides {
                Sides::D3 => self.d3() as i32,
                Sides::D6 => self.d6() as i32,
            };
        }
        total.max(0) as u32
    }
}

/// SplitMix64. Small, fast, good enough for dice, and trivially portable to
/// wasm. The seed and counter together are the whole state, so the server and
/// a replaying client stay in lockstep.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rng {
    /// Serialised as a decimal string: JavaScript numbers cannot hold a u64.
    #[serde(with = "u64_string")]
    state: u64,
    #[serde(with = "u64_string")]
    pub draws: u64,
}

mod u64_string {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(v: &u64, s: S) -> Result<S::Ok, S::Error> {
        v.to_string().serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Either {
            Text(String),
            Number(u64),
        }
        match Either::deserialize(d)? {
            Either::Text(t) => t.parse().map_err(serde::de::Error::custom),
            Either::Number(n) => Ok(n),
        }
    }
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng { state: seed, draws: 0 }
    }

    fn next_u64(&mut self) -> u64 {
        self.draws += 1;
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

impl Dice for Rng {
    fn d6(&mut self) -> u8 {
        (self.next_u64() % 6) as u8 + 1
    }
}

/// A fixed sequence of D6 results for tests. Panics if exhausted, so a test
/// that rolls more dice than it scripted fails loudly.
pub struct Scripted {
    rolls: std::vec::IntoIter<u8>,
}

impl Scripted {
    pub fn new(rolls: impl Into<Vec<u8>>) -> Self {
        Scripted { rolls: rolls.into().into_iter() }
    }
}

impl Dice for Scripted {
    fn d6(&mut self) -> u8 {
        self.rolls.next().expect("scripted dice exhausted")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Sides {
    D3,
    D6,
}

/// A dice expression such as `D6`, `2D6+1`, `D3` or a flat `3`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiceExpr {
    pub count: u32,
    pub sides: Sides,
    pub add: i32,
}

impl DiceExpr {
    pub const fn flat(n: i32) -> Self {
        DiceExpr { count: 0, sides: Sides::D6, add: n }
    }

    pub const fn d6(count: u32) -> Self {
        DiceExpr { count, sides: Sides::D6, add: 0 }
    }

    pub const fn d3(count: u32) -> Self {
        DiceExpr { count, sides: Sides::D3, add: 0 }
    }

    pub fn is_flat(&self) -> bool {
        self.count == 0
    }

    pub fn max(&self) -> u32 {
        let per = match self.sides {
            Sides::D3 => 3,
            Sides::D6 => 6,
        };
        (self.count as i32 * per + self.add).max(0) as u32
    }

    pub fn parse(s: &str) -> Result<Self, ParseError> {
        let s = s.trim().to_ascii_uppercase().replace(' ', "");
        if let Ok(n) = s.parse::<i32>() {
            return Ok(Self::flat(n));
        }
        let (dice, add) = match s.split_once('+') {
            Some((d, a)) => (d, a.parse::<i32>().map_err(|_| ParseError(s.clone()))?),
            None => match s.rsplit_once('-') {
                Some((d, a)) if !d.is_empty() => {
                    (d, -a.parse::<i32>().map_err(|_| ParseError(s.clone()))?)
                }
                _ => (s.as_str(), 0),
            },
        };
        let (count, sides) = if let Some(c) = dice.strip_suffix("D6") {
            (c, Sides::D6)
        } else if let Some(c) = dice.strip_suffix("D3") {
            (c, Sides::D3)
        } else {
            return Err(ParseError(s.clone()));
        };
        let count = if count.is_empty() {
            1
        } else {
            count.parse::<u32>().map_err(|_| ParseError(s.clone()))?
        };
        Ok(DiceExpr { count, sides, add })
    }
}

impl fmt::Display for DiceExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_flat() {
            return write!(f, "{}", self.add);
        }
        if self.count > 1 {
            write!(f, "{}", self.count)?;
        }
        match self.sides {
            Sides::D3 => write!(f, "D3")?,
            Sides::D6 => write!(f, "D6")?,
        }
        match self.add.signum() {
            1 => write!(f, "+{}", self.add),
            -1 => write!(f, "{}", self.add),
            _ => Ok(()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError(pub String);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "not a dice expression: {}", self.0)
    }
}

impl std::error::Error for ParseError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_rolls() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        let ra: Vec<u8> = (0..100).map(|_| a.d6()).collect();
        let rb: Vec<u8> = (0..100).map(|_| b.d6()).collect();
        assert_eq!(ra, rb);
        assert_eq!(a.draws, 100);
    }

    #[test]
    fn d6_in_range_and_roughly_uniform() {
        let mut rng = Rng::new(7);
        let mut counts = [0u32; 7];
        for _ in 0..60_000 {
            let r = rng.d6();
            assert!((1..=6).contains(&r));
            counts[r as usize] += 1;
        }
        for c in &counts[1..] {
            assert!((9_000..11_000).contains(c), "skewed: {counts:?}");
        }
    }

    #[test]
    fn parses_expressions() {
        assert_eq!(DiceExpr::parse("D6").unwrap(), DiceExpr::d6(1));
        assert_eq!(DiceExpr::parse("2D6+1").unwrap(), DiceExpr { count: 2, sides: Sides::D6, add: 1 });
        assert_eq!(DiceExpr::parse("d3").unwrap(), DiceExpr::d3(1));
        assert_eq!(DiceExpr::parse("D3+2").unwrap(), DiceExpr { count: 1, sides: Sides::D3, add: 2 });
        assert_eq!(DiceExpr::parse("3").unwrap(), DiceExpr::flat(3));
        assert_eq!(DiceExpr::parse("2D6-1").unwrap(), DiceExpr { count: 2, sides: Sides::D6, add: -1 });
        assert!(DiceExpr::parse("D20").is_err());
        assert!(DiceExpr::parse("lots").is_err());
    }

    #[test]
    fn displays_round_trip() {
        for s in ["D6", "2D6+1", "D3", "D3+2", "3", "2D6-1"] {
            assert_eq!(DiceExpr::parse(s).unwrap().to_string(), s);
        }
    }

    #[test]
    fn rng_round_trips_through_json_as_strings() {
        let mut r = Rng::new(u64::MAX - 7);
        r.d6();
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains('"'), "{json}");
        let back: Rng = serde_json::from_str(&json).unwrap();
        assert_eq!(r, back);
    }

    #[test]
    fn scripted_rolls_in_order() {
        let mut d = Scripted::new([6, 1, 3]);
        assert_eq!(d.roll(&DiceExpr::parse("2D6+1").unwrap()), 8);
        assert_eq!(d.d3(), 2);
    }
}
