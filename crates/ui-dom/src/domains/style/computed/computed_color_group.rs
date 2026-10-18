//! Inherited color computed group (PLAN.md S1.1).
//!
//! This file owns the `ComputedColorGroup` struct: the computed value of the
//! inherited `color` longhand. It is plain `Copy` data plus `initial()`;
//! resolution lives in the step-3.3 resolver.
//!
//! Invariants and rules:
//!
//! - `color_rgba` is the resolved text color as `[r, g, b, a]`
//! - `initial()` does **not** use the CSS initial (`canvastext`, i.e. opaque
//!   black): it mirrors what today's text paint actually defaults to when
//!   `color` is absent — the light gray
//!   `[240/255, 240/255, 240/255, 1.0]` (a near-white), not black. That is the
//!   same value the former `domains/paint/mod.rs::default_text_color_rgba`
//!   returned before spine step 3.3d sourced the painted text color from this
//!   group; the paint helper was deleted in that step and the value lives here
//!   as the single definition-site.

/// Today's absent-`color` text-paint default: the light gray
/// `[240/255, 240/255, 240/255, 1.0]`. This is what paint emits for a node
/// with no authored/inherited `color`, so the computed color group's initial
/// must match it exactly to keep observable output identical after wiring.
const DEFAULT_TEXT_COLOR_RGBA: [f32; 4] = [240.0 / 255.0, 240.0 / 255.0, 240.0 / 255.0, 1.0];

/// Computed value of the inherited `color` longhand.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ComputedColorGroup {
    /// Resolved text color.
    pub color_rgba: [f32; 4],
}

impl ComputedColorGroup {
    /// The no-parent color seed: today's absent-`color` paint default.
    #[must_use]
    pub fn initial() -> Self {
        Self {
            color_rgba: DEFAULT_TEXT_COLOR_RGBA,
        }
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)] // initial-value assertions compare exact literals
mod tests {
    use super::*;

    #[test]
    fn initial_mirrors_todays_absent_color_paint_default() {
        let color = ComputedColorGroup::initial();
        assert_eq!(
            color.color_rgba,
            [240.0 / 255.0, 240.0 / 255.0, 240.0 / 255.0, 1.0]
        );
        assert_eq!(color.color_rgba[3], 1.0);
    }
}
