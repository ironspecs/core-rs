//! Cascade tier ladder for the `ui-dom` style system.
//!
//! This file owns `CascadeTier`, the projection of CSS Cascade 5 §6.1's
//! origin-and-importance ladder onto the first (and therefore first-compared)
//! field of the `Precedence` record. It is a plain enum with no behavior; the
//! ordering it carries is the derived `Ord` over variant declaration order.
//!
//! Invariants and rules:
//!
//! - variants are declared in ASCENDING cascade order — the first variant is
//!   the LOWEST rung — because `Precedence` derives `Ord` (field order = sort
//!   order, `tier` first) and the winners fold's `precedence >= existing`
//!   comparison treats a larger `Precedence` as the winner
//! - stages 2–4 of cascade tier path F extend this enum additively by appending
//!   the next-higher rung (`ImportantUa` above, `Animation` between
//!   `NormalAuthor` and `ImportantAuthor`): the semantics of the existing
//!   variants never change, only the ladder grows taller

/// One rung of the cascade ladder (CSS Cascade 5 §6.1). Declared ascending so
/// the derived `Ord` sorts the important rung highest: `NormalUa` (the behavior
/// sheet) < `NormalAuthor` < `ImportantAuthor`.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum CascadeTier {
    /// Normal user-agent-ish declarations (the behavior sheet) — the lowest
    /// rung; every author declaration (normal or important) beats it. The
    /// behavior sheet's important counterpart (`ImportantUa`) arrives with
    /// stage 2.
    NormalUa,
    /// Normal author declarations (rules and normal inline style).
    NormalAuthor,
    /// Important author declarations (incl. important inline style) — beat all
    /// normal declarations (§6.1 rung 4).
    ImportantAuthor,
}
