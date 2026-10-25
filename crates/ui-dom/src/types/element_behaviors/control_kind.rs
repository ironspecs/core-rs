//! The closed set of control kinds a DOM element can resolve to.
//!
//! This file owns the behavior-table KEY type (`ControlKind`) and its
//! exhaustive census. It is deliberately finer than the legacy per-seam enums
//! (`DomElementOnKeyboard::InputControl` covers four kinds here) so that each
//! kind's row is a single dispatch shape.
//!
//! Invariants and rules:
//!
//! - `ControlKind` is the single behavior KEY: `resolve_kind` (in
//!   `behavior_table.rs`) maps an element to its kind.
//! - `ControlKind::ALL` is the exhaustive census of every variant — the source
//!   of truth the census test iterates, so a variant added without a table row
//!   fails the test. The exhaustive `match` in `behavior_row` remains the real
//!   compile-time guard; `ALL` is the second belt.
//! - This file has no behavior methods (the `ALL` impl is a const only), so it
//!   carries no unit tests of its own; its contract is pinned by the census
//!   test in `behavior_table.rs`.

/// The closed set of control kinds a DOM element can resolve to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ControlKind {
    Button,
    Checkbox,
    Number,
    Range,
    TextInput,
    Textarea,
    Select,
}

impl ControlKind {
    /// The exhaustive census of control kinds, in declaration order.
    ///
    /// The array order must match the `BEHAVIOR_TABLE` order and the
    /// declaration order above; `behavior_row` indexes by `match` so the table
    /// and the enum cannot drift without a compile error, and the census test
    /// iterates this array so a kind missing a row fails the test too.
    pub(crate) const ALL: [ControlKind; 7] = [
        ControlKind::Button,
        ControlKind::Checkbox,
        ControlKind::Number,
        ControlKind::Range,
        ControlKind::TextInput,
        ControlKind::Textarea,
        ControlKind::Select,
    ];
}
