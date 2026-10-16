//! Pure, deterministic rules engine.
//!
//! Nothing in this crate names a faction, a unit or a specific ability. Rules
//! that the publisher prints arrive as content; this crate only knows how to
//! interpret them. See `docs/design.md` at the repo root.

pub mod attack;
pub mod dice;
