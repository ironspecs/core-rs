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
//!   (D4 rule 2's order); a name matching multiple lines reports the count so
//!   the caller can apply the §7.1 multiple-same-name span rule

use crate::domains::style::computed::{
    AreaRect, ComputedGridGroup, GridTemplateAreas, GridTrackList,
};
use crate::domains::style::{DeclStore, TextInterner, TextKey};

/// Which axis a line resolution targets (the named-line maps are per-axis).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GridAxis {
    Column,
    Row,
}

/// The container's placement-name resolution context (D3/D4): the explicit
/// `[name]` line registrations from the two track lists plus the areas map (for
/// the area-rect shortcut and the implied `<name>-start`/`<name>-end` lines).
pub(crate) struct GridLineContext<'a> {
    computed_texts: &'a TextInterner,
    inline_store: &'a DeclStore,
    columns: Option<&'a GridTrackList>,
    rows: Option<&'a GridTrackList>,
    areas: Option<&'a GridTemplateAreas>,
}

impl<'a> GridLineContext<'a> {
    /// Builds the context from the container's resolved group, borrowing only
    /// the computed-text interner and the inline store (disjoint from the
    /// layout scratch the placement passes mutate).
    pub(crate) fn new(
        computed_texts: &'a TextInterner,
        inline_store: &'a DeclStore,
        grid_group: &ComputedGridGroup,
    ) -> Self {
        Self {
            computed_texts,
            inline_store,
            columns: inline_store.resolve_track_list(grid_group.template_columns),
            rows: inline_store.resolve_track_list(grid_group.template_rows),
            areas: inline_store.resolve_grid_areas(grid_group.template_areas),
        }
    }

    /// Resolves a referenced name to its matching line indices in `axis`:
    /// `(first_line, match_count)` — `None` first means no match. Explicit
    /// `[name]` lines are considered first, then the area-implied
    /// `<name>-start`/`<name>-end` lines (D4 rule 2's order).
    pub(crate) fn resolve_named(&self, name: TextKey, axis: GridAxis) -> (Option<u32>, u32) {
        let Some(name_str) = self.computed_texts.resolve(name) else {
            return (None, 0);
        };
        let mut first = None;
        let mut count = 0u32;
        let list = match axis {
            GridAxis::Column => self.columns,
            GridAxis::Row => self.rows,
        };
        if let Some(list) = list {
            for (key, index) in &list.line_names {
                if self.inline_store.resolve(*key) == Some(name_str) {
                    first.get_or_insert(u32::from(*index));
                    count += 1;
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
                first.get_or_insert(u32::from(line));
                count += 1;
            }
        }
        (first, count)
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
