//! Grid placement-name resolution context (`GridAxis` + `GridLineContext`).
//!
//! This file owns the container's D4 name-resolution context (grid T3,
//! `GRID_STEP4_DESIGN.md` D4): the axis enum and the context that resolves a
//! referenced name against the explicit `[name]` line registrations (from the
//! two track lists) and the areas map (the area-rect shortcut and the implied
//! `<name>-start`/`<name>-end` lines). It is used by the placement passes in
//! `grid.rs`; the resolution itself (the `LoweredLine` lowering, the
//! `ResolvedAxis` pair logic) stays in `grid.rs`.
//!
//! Invariants and rules:
//!
//! - name comparison is content equality: the referenced name (computed-text
//!   relative) resolves to `&str` and is compared against the arena names
//!   (inline-store relative), allocation-free and hash-free
//! - the explicit `[name]` lines are considered before the area-implied lines
//!   (D4 rule 2's order); a name matching multiple lines resolves to the FIRST
//!   match — the named set is indexed by filtering the placement by name
//!   (CSS Grid 1 §8.1.3), never a span across the matches
//! - the explicit name lists it resolves are the LAYOUT-TIME EXPANDED names
//!   (grid T3.5): an auto-fill/fit span's names sit at their expanded line
//!   positions (the suffix names shift right by `(N−1)·span_count`; the span's
//!   first-copy names keep their stored positions, which are already the first
//!   match) — the expansion is what name resolution sees, never the stored
//!   single copy

use crate::domains::style::computed::{AreaRect, GridTemplateAreas};
use crate::domains::style::{DeclStore, TextInterner, TextKey};

/// Which axis a line resolution targets (the named-line maps are per-axis).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GridAxis {
    Column,
    Row,
}

/// The container's placement-name resolution context (D3/D4): the EXPANDED
/// explicit `[name]` line registrations from the two track lists plus the areas
/// map (for the area-rect shortcut and the implied `<name>-start`/`<name>-end`
/// lines). The line-name slices are the layout-time expansion of the retained
/// track lists (grid T3.5) — `resolve_named` sees the expanded positions.
pub(crate) struct GridLineContext<'a> {
    computed_texts: &'a TextInterner,
    inline_store: &'a DeclStore,
    columns: Option<&'a [(TextKey, u8)]>,
    rows: Option<&'a [(TextKey, u8)]>,
    areas: Option<&'a GridTemplateAreas>,
}

impl<'a> GridLineContext<'a> {
    /// Builds the context from the resolved group, borrowing only the
    /// computed-text interner, the inline store, the EXPANDED column/row
    /// line-name slices, and the areas map (disjoint from the layout scratch
    /// the placement passes mutate).
    pub(crate) fn new(
        computed_texts: &'a TextInterner,
        inline_store: &'a DeclStore,
        columns: Option<&'a [(TextKey, u8)]>,
        rows: Option<&'a [(TextKey, u8)]>,
        areas: Option<&'a GridTemplateAreas>,
    ) -> Self {
        Self {
            computed_texts,
            inline_store,
            columns,
            rows,
            areas,
        }
    }

    /// Resolves a referenced name to its FIRST matching line index in `axis`
    /// (`None` when no line matches). Explicit `[name]` lines are considered
    /// first, then the area-implied `<name>-start`/`<name>-end` lines (D4 rule
    /// 2's order). Multiple same-name lines form a named set indexed by
    /// filtering the placement by name (CSS Grid 1 §8.1.3): the bare
    /// `<custom-ident>` form is the first match, never a span across matches.
    pub(crate) fn resolve_named(&self, name: TextKey, axis: GridAxis) -> Option<u32> {
        let name_str = self.computed_texts.resolve(name)?;
        let names = match axis {
            GridAxis::Column => self.columns,
            GridAxis::Row => self.rows,
        };
        if let Some(names) = names {
            for (key, index) in names {
                if self.inline_store.resolve(*key) == Some(name_str) {
                    return Some(u32::from(*index));
                }
            }
        }
        if let Some(areas) = self.areas
            && let Some((base, is_start)) = split_area_line_name(name_str)
        {
            for (area_key, rect) in &areas.names {
                if self.inline_store.resolve(*area_key) != Some(base) {
                    continue;
                }
                let line = match (axis, is_start) {
                    (GridAxis::Column, true) => rect.min_col,
                    (GridAxis::Column, false) => rect.max_col + 1,
                    (GridAxis::Row, true) => rect.min_row,
                    (GridAxis::Row, false) => rect.max_row + 1,
                };
                return Some(u32::from(line));
            }
        }
        None
    }

    /// Looks up `name` as a named AREA, returning its rect (D4 rule 1).
    pub(crate) fn resolve_area(&self, name: TextKey) -> Option<AreaRect> {
        let name_str = self.computed_texts.resolve(name)?;
        self.areas.and_then(|areas| {
            areas
                .names
                .iter()
                .find(|(key, _)| self.inline_store.resolve(*key) == Some(name_str))
                .map(|(_, rect)| *rect)
        })
    }
}

/// Splits an implied area-line name into `(area, is_start)`: `foo-start` →
/// `("foo", true)`, `foo-end` → `("foo", false)`. `None` for any other name.
pub(crate) fn split_area_line_name(name: &str) -> Option<(&str, bool)> {
    if let Some(base) = name.strip_suffix("-start") {
        Some((base, true))
    } else if let Some(base) = name.strip_suffix("-end") {
        Some((base, false))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_area_line_name_recognizes_start_and_end_suffixes() {
        assert_eq!(split_area_line_name("header-start"), Some(("header", true)));
        assert_eq!(split_area_line_name("header-end"), Some(("header", false)));
        assert_eq!(split_area_line_name("header"), None);
        assert_eq!(split_area_line_name("start"), None);
    }
}
