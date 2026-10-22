//! Interned grid track-list computed value (`GridTrackList`).
//!
//! This file owns the track-list arena's value type (grid T3,
//! `GRID_STEP4_DESIGN.md` D3 — the storage representation landed under the
//! transcription's STOP adjudication). It holds the expanded track sizes plus
//! the retained `[name]` line-name registrations, so the track-list wire format
//! can round-trip brackets and placement can resolve `Named`/`NamedSpan` line
//! references against explicit names.
//!
//! It is plain `Copy` data plus two behavior methods — name registration (the
//! capacity and per-name-count gates) and canonical serialization (the bracket
//! round-trip). Track sizing itself lives in `grid.rs`; the `GridTrackSize`
//! leaf vocabulary lives in `grid_track_size.rs`; the single-size formatter the
//! serializer delegates to lives in `decl_value.rs`.
//!
//! Invariants and rules:
//!
//! - `tracks` holds at most `GRID_TRACK_LIST_CAPACITY` (12) expanded tracks; a
//!   list over capacity drops at parse, never a silent cap
//! - `line_names` holds at most `GRID_LINE_NAME_CAPACITY` (8) `(name_key,
//!   line_index)` registrations; a bracket group attaches to the line at its
//!   position (the line BEFORE the following track), so `line_index ==
//!   tracks.len()` at registration time; a trailing bracket names the final
//!   line at index `tracks.len()`
//! - a single name labels at most `GRID_LINE_NAME_PER_NAME_CAPACITY` (4) lines
//!   in one list; the cap bounds a name's legitimate multiplicity for the
//!   named set's Nth-match indexing (CSS Grid §8.1.3 "exclusively indexed by
//!   filtering the placement by name" — `text 5` is the 5th line named
//!   `text`); the fifth registration drops + counts at parse
//! - value-dedup interning compares FULL content (tracks + name pairs), so two
//!   identical named templates intern to one key

use arrayvec::ArrayVec;

use super::grid_track_size::GridTrackSize;
use crate::domains::style::decl_value::{TextKey, format_grid_track_size};

/// The maximum expanded track count a grid track list may hold (the design-
/// system standard; larger lists are a named D6-gated follow-up).
pub(crate) const GRID_TRACK_LIST_CAPACITY: usize = 12;

/// The maximum `(name, line_index)` registrations one track list retains.
pub(crate) const GRID_LINE_NAME_CAPACITY: usize = 8;

/// The maximum lines one name may label within one track list. A name may
/// legitimately label multiple lines for the named set's Nth-match indexing
/// (CSS Grid §8.1.3 "exclusively indexed by filtering the placement by name" —
/// `text 5` is the 5th line named `text`); the cap bounds that multiplicity,
/// and anything beyond it drops with a counted report.
pub(crate) const GRID_LINE_NAME_PER_NAME_CAPACITY: usize = 4;

/// An interned grid track list: the expanded track sizes plus the retained
/// `[name]` line-name registrations.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GridTrackList {
    /// The expanded track sizes, in order.
    pub(crate) tracks: ArrayVec<GridTrackSize, GRID_TRACK_LIST_CAPACITY>,
    /// `(name_key, line_index)` — a bracket group's names attach to the line
    /// before the following track (a trailing bracket names the final line).
    pub(crate) line_names: ArrayVec<(TextKey, u8), GRID_LINE_NAME_CAPACITY>,
}

impl GridTrackList {
    /// Registers one line name at `line_index`, failing (whole-list drop at
    /// parse) when the list already holds `GRID_LINE_NAME_CAPACITY`
    /// registrations or the name already labels `GRID_LINE_NAME_PER_NAME_CAPACITY`
    /// lines.
    #[must_use]
    pub(crate) fn register_line_name(&mut self, name: TextKey, line_index: u8) -> bool {
        if self.line_names.len() >= GRID_LINE_NAME_CAPACITY {
            return false;
        }
        let occurrences = self
            .line_names
            .iter()
            .filter(|(existing, _)| *existing == name)
            .count();
        if occurrences >= GRID_LINE_NAME_PER_NAME_CAPACITY {
            return false;
        }
        self.line_names.push((name, line_index));
        true
    }

    /// Serializes the list to its canonical CSS token form, re-inserting the
    /// `[name1 name2]` brackets before (and after) the tracks they attach to.
    /// `resolve` maps a `TextKey` name to its string (a self-owned structural
    /// invariant — a name key must resolve in its owning store).
    #[must_use]
    pub(crate) fn to_css_string<'a>(&self, resolve: impl Fn(TextKey) -> Option<&'a str>) -> String {
        let mut out = String::new();
        let line_count = self.tracks.len();
        for line in 0..=line_count {
            let names: Vec<&str> = self
                .line_names
                .iter()
                .filter(|(_, index)| usize::from(*index) == line)
                .filter_map(|(key, _)| resolve(*key))
                .collect();
            if !names.is_empty() {
                out.push('[');
                out.push_str(&names.join(" "));
                out.push_str("] ");
            }
            if line < line_count {
                out.push_str(&format_grid_track_size(&self.tracks[line]));
                out.push(' ');
            }
        }
        out.trim_end().to_string()
    }
}

#[cfg(test)]
use crate::domains::style::DeclStore;
#[cfg(test)]
use crate::domains::style::computed::{TrackBreadth, TrackMaxBreadth};

#[cfg(test)]
#[allow(clippy::float_cmp)] // track-size literals are exact in these assertions
mod tests {
    use super::*;

    fn px(min: f32, max: f32) -> GridTrackSize {
        GridTrackSize {
            min: TrackBreadth::Px(min),
            max: TrackMaxBreadth::Px(max),
        }
    }

    #[test]
    fn register_line_name_gates_per_name_count() {
        let mut list = GridTrackList::default();
        let mut names = Vec::new();
        for _ in 0..4 {
            names.push(TextKey(u32::try_from(names.len()).unwrap()));
        }
        // Four registrations of one name are admitted.
        for index in 0..4u8 {
            assert!(list.register_line_name(names[0], index));
        }
        // The fifth of the SAME name drops.
        assert!(!list.register_line_name(names[0], 4));
        // A distinct name still admits (no cross-name interference).
        assert!(list.register_line_name(names[1], 4));
        assert_eq!(list.line_names.len(), 5);
    }

    #[test]
    fn register_line_name_capacity_eight() {
        let mut list = GridTrackList::default();
        for index in 0..8u32 {
            assert!(
                list.register_line_name(
                    TextKey(index),
                    u8::try_from(index).expect("index fits u8")
                )
            );
        }
        assert!(!list.register_line_name(TextKey(8), 8));
        assert_eq!(list.line_names.len(), 8);
    }

    #[test]
    fn serialization_round_trips_brackets() {
        let mut list = GridTrackList::default();
        let mut store = DeclStore::default();
        list.tracks.push(px(100.0, 100.0));
        list.tracks.push(px(200.0, 200.0));
        let main = store.intern("main");
        let aside = store.intern("aside");
        assert!(list.register_line_name(main, 0));
        assert!(list.register_line_name(aside, 1));
        let serialized = list.to_css_string(|key| store.resolve(key));
        assert_eq!(serialized, "[main] 100px [aside] 200px");
    }

    #[test]
    fn trailing_bracket_names_the_final_line() {
        let mut list = GridTrackList::default();
        let mut store = DeclStore::default();
        list.tracks.push(px(100.0, 100.0));
        let end = store.intern("end");
        assert!(list.register_line_name(end, 1));
        let serialized = list.to_css_string(|key| store.resolve(key));
        assert_eq!(serialized, "100px [end]");
    }
}
