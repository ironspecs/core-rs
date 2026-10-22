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
//! - value-dedup interning compares FULL content (tracks + name pairs + the
//!   auto-repeat span), so two identical named templates intern to one key
//! - `auto_repeat_span` is `Some((start, count, kind))` for an auto-fill/fit
//!   list: the span's inner tracks are stored as ONE copy at `tracks[start ..
//!   start+count]` (prefix, span, suffix), and only `AutoFill`/`AutoFit` are
//!   ever stored in it (a fixed `repeat(N, …)` expands at parse, leaving the
//!   field `None`). Named lines inside the span are the registrations with a
//!   `line_index` in `start..=start+count`; they repeat with their tracks when
//!   the span expands at layout time (the suffix names shift right by
//!   `(N−1)·count`)

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

/// The repetition kind a grid track list may carry (grid T3.5, D1).
///
/// `None` and `Count(u8)` are declared to close the enum's shape but are never
/// stored in `auto_repeat_span`: a fixed `repeat(N, …)` keeps its parse-time
/// expansion (so the span stays `None`), and the span field only ever carries
/// `AutoFill`/`AutoFit`.
#[allow(dead_code)] // `None`/`Count` close the enum shape; only AutoFill/AutoFit are span values
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GridRepeat {
    /// No repetition (a fully expanded list).
    None,
    /// A fixed repetition count (declared; fixed repeats expand at parse).
    Count(u8),
    /// `repeat(auto-fill, <track-list>)` — repetition count resolved at layout.
    AutoFill,
    /// `repeat(auto-fit, <track-list>)` — like `auto-fill` plus empty-track
    /// collapse after placement.
    AutoFit,
}

/// The canonical `repeat(…)` keyword prefix for a span kind (the serialization
/// helper). Only the two auto-repeat kinds reach here (a self-owned structural
/// invariant — the span is built by the parser, never by boundary data).
fn repeat_keyword(kind: GridRepeat) -> &'static str {
    match kind {
        GridRepeat::AutoFill => "repeat(auto-fill, ",
        GridRepeat::AutoFit => "repeat(auto-fit, ",
        GridRepeat::None | GridRepeat::Count(_) => {
            unreachable!("auto_repeat_span only ever carries AutoFill/AutoFit")
        }
    }
}

/// An interned grid track list: the expanded track sizes plus the retained
/// `[name]` line-name registrations, and (for an auto-fill/fit list) the
/// auto-repeat span marker.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GridTrackList {
    /// The track sizes, in order. For an auto-repeat list this holds the prefix
    /// tracks, ONE copy of the span's inner tracks, then the suffix tracks.
    pub(crate) tracks: ArrayVec<GridTrackSize, GRID_TRACK_LIST_CAPACITY>,
    /// `(name_key, line_index)` — a bracket group's names attach to the line
    /// before the following track (a trailing bracket names the final line).
    pub(crate) line_names: ArrayVec<(TextKey, u8), GRID_LINE_NAME_CAPACITY>,
    /// The auto-fill/fit span marker: `(start_track_index, inner_track_count,
    /// kind)` where the span's inner list is stored once at
    /// `tracks[start..start+count]`. `None` for a fully expanded list.
    pub(crate) auto_repeat_span: Option<(u8, u8, GridRepeat)>,
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
    /// `[name1 name2]` brackets before (and after) the tracks they attach to,
    /// and re-wrapping an auto-fill/fit span in its `repeat(auto-*, …)` form.
    /// `resolve` maps a `TextKey` name to its string (a self-owned structural
    /// invariant — a name key must resolve in its owning store).
    #[must_use]
    pub(crate) fn to_css_string<'a>(&self, resolve: impl Fn(TextKey) -> Option<&'a str>) -> String {
        let mut out = String::new();
        let line_count = self.tracks.len();
        let span = self.auto_repeat_span;
        let mut line = 0usize;
        while line <= line_count {
            if let Some((start, count, kind)) = span
                && line == usize::from(start)
            {
                // Emit the span as `repeat(auto-*, <inner list>)`: the inner
                // list is the span's one stored copy (its tracks plus the
                // names at its lines).
                out.push_str(repeat_keyword(kind));
                let end = line + usize::from(count);
                for inner in line..=end {
                    self.push_line_names(&mut out, inner, &resolve);
                    if inner < end {
                        out.push_str(&format_grid_track_size(&self.tracks[inner]));
                        out.push(' ');
                    }
                }
                // Drop the separator space preceding the closing `)`.
                while out.ends_with(' ') {
                    out.pop();
                }
                out.push(')');
                out.push(' ');
                line = end;
                continue;
            }
            self.push_line_names(&mut out, line, &resolve);
            if line < line_count {
                out.push_str(&format_grid_track_size(&self.tracks[line]));
                out.push(' ');
            }
            line += 1;
        }
        out.trim_end().to_string()
    }

    /// Appends the `[name1 name2]` bracket group for `line` (if any) to `out`.
    fn push_line_names<'a>(
        &self,
        out: &mut String,
        line: usize,
        resolve: &impl Fn(TextKey) -> Option<&'a str>,
    ) {
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

    #[test]
    fn auto_fit_span_round_trips_repeat_wrapper_and_inner_names() {
        // `repeat(auto-fit, [main] minmax(240px, 1fr))` stores one span copy
        // with the `[main]` line name inside, and serializes back to the
        // `repeat(auto-fit, …)` form.
        let mut list = GridTrackList::default();
        let mut store = DeclStore::default();
        list.tracks.push(GridTrackSize {
            min: TrackBreadth::Px(240.0),
            max: TrackMaxBreadth::Fr(1.0),
        });
        let main = store.intern("main");
        assert!(list.register_line_name(main, 0));
        list.auto_repeat_span = Some((0, 1, GridRepeat::AutoFit));
        let serialized = list.to_css_string(|key| store.resolve(key));
        assert_eq!(serialized, "repeat(auto-fit, [main] minmax(240px, 1fr))");
    }

    #[test]
    fn auto_fill_span_with_prefix_and_suffix_round_trips() {
        // `100px repeat(auto-fill, 200px) 300px` — the span is the middle track.
        let mut list = GridTrackList::default();
        list.tracks.push(px(100.0, 100.0));
        list.tracks.push(px(200.0, 200.0));
        list.tracks.push(px(300.0, 300.0));
        list.auto_repeat_span = Some((1, 1, GridRepeat::AutoFill));
        let store = DeclStore::default();
        let serialized = list.to_css_string(|key| store.resolve(key));
        assert_eq!(serialized, "100px repeat(auto-fill, 200px) 300px");
    }

    #[test]
    fn default_list_has_no_auto_repeat_span() {
        let list = GridTrackList::default();
        assert_eq!(list.auto_repeat_span, None);
    }
}
