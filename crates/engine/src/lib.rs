//! Pure, deterministic rules engine.
//!
//! Nothing in this crate names a faction, a unit or a specific ability. Rules
//! that the publisher prints arrive as content; this crate only knows how to
//! interpret them. See `docs/design.md` at the repo root.

pub mod actions;
pub mod attack;
pub mod combat;
pub mod content;
pub mod demo;
pub mod dice;
pub mod effects;
pub mod geom;
pub mod rules;
pub mod state;

pub use actions::{apply, Action, RuleError};
pub use state::GameState;
