//! Anonymous inline-run flex item formation (flex arc step 3a-ii).
//!
//! What this file owns:
//!
//! - `RunAccumulator`, the D3 fold accumulator: the three content cursors
//!   (`widest_segment_px`, `max_content_sum_px`, the trailing fragment) plus
//!   the whitespace-drop predicate data.
//! - `MemberMetrics`, one run member's fragment-level intrinsic as the fold
//!   consumes it (text nodes read the retained `DomIntrinsic::Text`; an inline
//!   element is ONE ATOM whose min/max come from its measurement record and
//!   whose edges are BREAKS).
//! - the fold mechanics as free functions (`empty_run`, `accumulate_run`,
//!   `member_metrics_from_text`, `member_metrics_from_element_atom`), the D1
//!   run-membership predicate, the D7 mode half, the D3 whitespace-drop mode
//!   gate, and the collapsible-whitespace character set.
//!
//! Invariants and rules:
//!
//! - the fold is pure: it reads each member's retained fragment surface (the
//!   style pass measured the text intrinsics; the measure walk, in `flex.rs`,
//!   measured the inline elements into their records before folding) — no text
//!   shaping, no allocation, no record writes in this file
//! - min-content is the widest unbreakable SEGMENT, joined across child
//!   boundaries only when no soft wrap opportunity intervenes (CSS Text 3 §4);
//!   max-content is a plain sum; the strut is the max fragment line-height
//! - a boundary is a soft-wrap opportunity iff the mode allows wrapping AND the
//!   boundary char is collapsible whitespace (D7): `edge_is_break(flag) =
//!   flag && mode_allows_wrapping`. An inline ELEMENT boundary is always a
//!   break (the D1 atom rule — no fragment joins across an element boundary)
//! - a run never manufactures a DOM object: it is a virtual item keyed by its
//!   first child (`first_child_key` + `child_count`); the run's children keep
//!   their DOM identity and are laid out during placement
//! - the whitespace-only flag is mode-independent (all characters are
//!   collapsible whitespace); the drop itself is conditioned on the container's
//!   white-space mode at the emit site (`flex.rs`), never here

use crate::types::dom_node::{DomIntrinsic, DomNode};
use crate::types::{DomNodeSlotKey, DomWhiteSpace};

use super::measurement_record::node_measurement_of;
use super::style::{LayoutDisplay, PositionMode, presented_display, presented_position};

/// The D3 fold accumulator for one maximal run of contiguous inline-level
/// in-flow children (CSS Flexbox §4 anonymous flex item).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct RunAccumulator {
    /// The run's identity anchor: the first child's slot key (children are
    /// contiguous siblings; `child_count` is the run's direct-child count).
    pub(super) first_child_key: DomNodeSlotKey,
    /// Number of direct children folded (never zero for a formed run).
    pub(super) child_count: u32,
    /// Cursor (iii): plain sum of members' max-content widths (no breaks).
    pub(super) max_content_sum_px: f32,
    /// Cursor (i): the widest unbreakable segment (may span child boundaries).
    /// This is the run's min-content.
    pub(super) widest_segment_px: f32,
    /// Cursor (ii): the run's current trailing unbreakable fragment width.
    pub(super) trailing_fragment_px: f32,
    /// May the NEXT member join to the trailing fragment? (`false` after a
    /// break-yielding trailing edge — a wrap-opportunity char or an element
    /// atom boundary.)
    pub(super) trailing_join_open: bool,
    /// Max `fragment_line_height_max` over members (the strut cross size).
    pub(super) strut_line_height_px: f32,
    /// Every member folded so far is a text node whose characters are all
    /// collapsible whitespace. The D3 emit gate reads this: an all-whitespace
    /// run is dropped when the container's white-space mode collapses spaces.
    pub(super) all_members_text_and_all_whitespace: bool,
}

/// One run member's fragment-level metrics, as the fold consumes them.
#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(clippy::struct_excessive_bools)] // the two edge classes are a closed, mirroring vocabulary
pub(super) struct MemberMetrics {
    pub(super) min_content_px: f32,
    pub(super) max_content_px: f32,
    pub(super) leading_fragment_px: f32,
    pub(super) trailing_fragment_px: f32,
    /// A collapsible whitespace/segment-break char at the leading edge (text
    /// nodes only; the D7 mode half is combined at fold time).
    pub(super) starts_with_wrap_opportunity: bool,
    /// A collapsible whitespace/segment-break char at the trailing edge (text
    /// nodes only; the D7 mode half is combined at fold time).
    pub(super) ends_with_wrap_opportunity: bool,
    /// The leading edge is an ELEMENT boundary (always a break, D1 atom rule).
    pub(super) leading_edge_is_element_break: bool,
    /// The trailing edge is an ELEMENT boundary (always a break, D1 atom rule).
    pub(super) trailing_edge_is_element_break: bool,
    /// The member's fragment line-height (the strut contribution).
    pub(super) line_height_px: f32,
    /// The member is a text node whose data is all collapsible whitespace.
    pub(super) is_whitespace_only: bool,
}

/// An empty run starting at `first_child_key`: vacuously whitespace-only, no
/// members yet.
pub(super) fn empty_run(first_child_key: DomNodeSlotKey) -> RunAccumulator {
    RunAccumulator {
        first_child_key,
        child_count: 0,
        max_content_sum_px: 0.0,
        widest_segment_px: 0.0,
        trailing_fragment_px: 0.0,
        trailing_join_open: false,
        strut_line_height_px: 0.0,
        all_members_text_and_all_whitespace: true,
    }
}

/// Folds one member's metrics into the run's cursors (the D3 arithmetic).
///
/// The join decision is `joined = !boundary_is_break` where
/// `boundary_is_break` is a break at either side: the previous member's
/// trailing edge (`!trailing_join_open`) or this member's leading edge (an
/// element boundary, or a collapsible-whitespace char when the mode allows
/// wrapping). Under `nowrap`/`pre` `mode_allows_wrapping` is false, so
/// whitespace boundaries never break and the whole run joins into one
/// unbreakable segment (D7).
pub(super) fn accumulate_run(run: &mut RunAccumulator, member: MemberMetrics, mode: DomWhiteSpace) {
    let this_leading_break = member.leading_edge_is_element_break
        || edge_is_break(member.starts_with_wrap_opportunity, mode);
    let this_trailing_break = member.trailing_edge_is_element_break
        || edge_is_break(member.ends_with_wrap_opportunity, mode);
    let boundary_is_break = !run.trailing_join_open || this_leading_break;
    let joined = !boundary_is_break;
    if run.child_count == 0 {
        run.widest_segment_px = member.min_content_px;
        run.trailing_fragment_px = member.trailing_fragment_px;
    } else if joined {
        run.widest_segment_px = run
            .widest_segment_px
            .max(run.trailing_fragment_px + member.leading_fragment_px)
            .max(member.min_content_px);
        run.trailing_fragment_px += member.trailing_fragment_px;
    } else {
        run.widest_segment_px = run.widest_segment_px.max(member.min_content_px);
        run.trailing_fragment_px = member.trailing_fragment_px;
    }
    run.trailing_join_open = !this_trailing_break;
    run.max_content_sum_px += member.max_content_px;
    run.strut_line_height_px = run.strut_line_height_px.max(member.line_height_px);
    run.all_members_text_and_all_whitespace &= member.is_whitespace_only;
    run.child_count += 1;
}

/// A text node's fragment metrics, read from its retained `DomIntrinsic::Text`
/// (the style pass measured it; the run fold never shapes text). Its edges are
/// whitespace edges (subject to the D7 mode half), never element edges.
pub(super) fn member_metrics_from_text(node: &DomNode) -> MemberMetrics {
    let DomIntrinsic::Text(text) = node.intrinsic() else {
        unreachable!("text nodes should carry a measured text intrinsic after the style pass");
    };
    let text_node = node
        .as_text()
        .expect("text intrinsic read should target a text node");
    MemberMetrics {
        min_content_px: text.min_content_width_px,
        max_content_px: text.max_content_width_px,
        leading_fragment_px: text.leading_fragment_width_px,
        trailing_fragment_px: text.trailing_fragment_width_px,
        starts_with_wrap_opportunity: text.starts_with_wrap_opportunity,
        ends_with_wrap_opportunity: text.ends_with_wrap_opportunity,
        leading_edge_is_element_break: false,
        trailing_edge_is_element_break: false,
        line_height_px: text.fragment_line_height_max_px,
        is_whitespace_only: text_is_all_collapsible_whitespace(text_node.data()),
    }
}

/// An inline ELEMENT member's metrics (the D1 atom rule): the crate's inline
/// intrinsic gives min/max (read from the measurement record the measure walk
/// just wrote) but no edge fragments, so both fragments are the whole
/// min-content and both edges are BREAKS — no fragment joins across an element
/// boundary. `line_height_px` is the member's measured height (the measure
/// walk's `FlowSize::height_px`).
pub(super) fn member_metrics_from_element_atom(
    node: &DomNode,
    line_height_px: f32,
) -> MemberMetrics {
    let record = node_measurement_of(node);
    let min_content_px = record.h_min_content_px;
    MemberMetrics {
        min_content_px,
        max_content_px: record.h_max_content_px,
        leading_fragment_px: min_content_px,
        trailing_fragment_px: min_content_px,
        starts_with_wrap_opportunity: false,
        ends_with_wrap_opportunity: false,
        leading_edge_is_element_break: true,
        trailing_edge_is_element_break: true,
        line_height_px,
        is_whitespace_only: false,
    }
}

/// Whether one node is a member of an inline run: `display: inline` (text
/// nodes and inline elements) and in-flow (static/relative). Block-level
/// children (block, inline-block, flex, inline-flex, img, element controls) and
/// abs-pos/`display: none` children are run-breakers.
pub(super) fn node_is_inline_run_member(node: &DomNode) -> bool {
    presented_display(node) == LayoutDisplay::Inline
        && matches!(
            presented_position(node),
            PositionMode::Static | PositionMode::Relative
        )
}

/// The D7 mode half: a boundary is a soft-wrap opportunity only when the mode
/// allows wrapping (`normal`, `pre-wrap`, `pre-line` — NOT `nowrap`, NOT
/// `pre`). The fold ANDs this with the 3a-i boundary-whitespace flags so
/// `nowrap`/`pre` force the boundary to never break.
pub(super) fn mode_allows_wrapping(white_space: DomWhiteSpace) -> bool {
    matches!(
        white_space,
        DomWhiteSpace::Normal | DomWhiteSpace::PreWrap | DomWhiteSpace::PreLine
    )
}

/// A whitespace-only run collapses to nothing (and is dropped) only under
/// `normal` or `pre-line`; under every other mode (`nowrap`, `pre`,
/// `pre-wrap`, `break-spaces`) its spaces keep width and the run forms an item.
pub(super) fn container_mode_is_collapse(white_space: DomWhiteSpace) -> bool {
    matches!(white_space, DomWhiteSpace::Normal | DomWhiteSpace::PreLine)
}

/// A boundary is a soft-wrap opportunity iff the boundary char is collapsible
/// whitespace (the 3a-i flag) AND the mode allows wrapping (D7).
fn edge_is_break(boundary_flag: bool, mode: DomWhiteSpace) -> bool {
    boundary_flag && mode_allows_wrapping(mode)
}

/// Whether every character is collapsible whitespace (empty is vacuously
/// whitespace-only). Matches the fragment-intrinsic boundary-whitespace set:
/// space, tab, segment breaks, form feed (CSS Text 3).
fn text_is_all_collapsible_whitespace(text: &str) -> bool {
    text.chars().all(is_collapsible_whitespace)
}

fn is_collapsible_whitespace(character: char) -> bool {
    matches!(character, ' ' | '\t' | '\n' | '\r' | '\u{000c}')
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    /// A text member with explicit fragment edges (the fold reads them blind).
    fn text_member(
        min: f32,
        max: f32,
        leading: f32,
        trailing: f32,
        starts: bool,
        ends: bool,
        whitespace_only: bool,
    ) -> MemberMetrics {
        MemberMetrics {
            min_content_px: min,
            max_content_px: max,
            leading_fragment_px: leading,
            trailing_fragment_px: trailing,
            starts_with_wrap_opportunity: starts,
            ends_with_wrap_opportunity: ends,
            leading_edge_is_element_break: false,
            trailing_edge_is_element_break: false,
            line_height_px: 14.0,
            is_whitespace_only: whitespace_only,
        }
    }

    /// An inline-element atom member: min/max from its record, edges broken.
    fn element_atom_member(min: f32, max: f32) -> MemberMetrics {
        MemberMetrics {
            min_content_px: min,
            max_content_px: max,
            leading_fragment_px: min,
            trailing_fragment_px: min,
            starts_with_wrap_opportunity: false,
            ends_with_wrap_opportunity: false,
            leading_edge_is_element_break: true,
            trailing_edge_is_element_break: true,
            line_height_px: 14.0,
            is_whitespace_only: false,
        }
    }

    fn new_slot_key() -> DomNodeSlotKey {
        let mut keys = slotmap::SlotMap::with_key();
        keys.insert(())
    }

    fn new_empty_run() -> RunAccumulator {
        empty_run(new_slot_key())
    }

    #[test]
    fn join_extends_the_trailing_fragment_across_the_boundary() {
        // "foo"+"bar" (no boundary whitespace): the fragments join, so the
        // min-content is the joined segment, not the widest single word.
        let mut run = new_empty_run();
        accumulate_run(
            &mut run,
            text_member(3.0, 3.0, 3.0, 3.0, false, false, false),
            DomWhiteSpace::Normal,
        );
        accumulate_run(
            &mut run,
            text_member(4.0, 4.0, 4.0, 4.0, false, false, false),
            DomWhiteSpace::Normal,
        );

        assert_eq!(run.widest_segment_px, 7.0);
        assert_eq!(run.max_content_sum_px, 7.0);
        assert_eq!(run.child_count, 2);
    }

    #[test]
    fn no_join_when_a_boundary_whitespace_breaks_the_run() {
        // "foo "+"bar": the trailing collapsible space is a soft-wrap
        // opportunity under `normal`, so the fragments do not join.
        let mut run = new_empty_run();
        accumulate_run(
            &mut run,
            text_member(3.0, 4.0, 3.0, 3.0, false, true, false),
            DomWhiteSpace::Normal,
        );
        accumulate_run(
            &mut run,
            text_member(4.0, 4.0, 4.0, 4.0, false, false, false),
            DomWhiteSpace::Normal,
        );

        assert_eq!(run.widest_segment_px, 4.0);
        assert_eq!(run.max_content_sum_px, 8.0);
    }

    #[test]
    fn nowrap_joins_a_whitespace_boundary_that_normal_would_break() {
        // Under `nowrap` the text intrinsic for "foo " is one unbreakable
        // fragment of the FULL width (min == max == w("foo ") == 4), and its
        // trailing whitespace flag is still set — but the mode forbids
        // wrapping, so `edge_is_break` is false and the boundary never breaks.
        // The whole run joins into one segment (min-content == max-content ==
        // sum == 8). Under `normal` the same boundary would break at 4.
        let mut run = new_empty_run();
        accumulate_run(
            &mut run,
            text_member(4.0, 4.0, 4.0, 4.0, false, true, false),
            DomWhiteSpace::Nowrap,
        );
        accumulate_run(
            &mut run,
            text_member(4.0, 4.0, 4.0, 4.0, false, false, false),
            DomWhiteSpace::Nowrap,
        );

        assert_eq!(run.widest_segment_px, 8.0);
        assert_eq!(run.max_content_sum_px, 8.0);
        assert_eq!(run.widest_segment_px, run.max_content_sum_px);
    }

    #[test]
    fn whitespace_only_drop_is_conditioned_on_the_collapse_mode() {
        let mut run = new_empty_run();
        accumulate_run(
            &mut run,
            text_member(0.0, 0.0, 0.0, 0.0, true, true, true),
            DomWhiteSpace::Normal,
        );
        accumulate_run(
            &mut run,
            text_member(0.0, 0.0, 0.0, 0.0, true, true, true),
            DomWhiteSpace::Normal,
        );

        // The run is all-whitespace; the D3 emit gate drops it under a
        // collapse mode and keeps it under a preserve/nowrap mode.
        assert!(run.all_members_text_and_all_whitespace);
        assert!(container_mode_is_collapse(DomWhiteSpace::Normal));
        assert!(container_mode_is_collapse(DomWhiteSpace::PreLine));
        assert!(!container_mode_is_collapse(DomWhiteSpace::Pre));
        assert!(!container_mode_is_collapse(DomWhiteSpace::PreWrap));
        assert!(!container_mode_is_collapse(DomWhiteSpace::Nowrap));
        assert!(!container_mode_is_collapse(DomWhiteSpace::BreakSpaces));

        // A non-whitespace member makes the run non-droppable.
        accumulate_run(
            &mut run,
            text_member(4.0, 4.0, 4.0, 4.0, false, false, false),
            DomWhiteSpace::Normal,
        );
        assert!(!run.all_members_text_and_all_whitespace);
    }

    #[test]
    fn element_atom_breaks_on_both_edges_and_never_joins() {
        // "a" + <atom 2px> + "b": the element's edges are BREAKS, so neither
        // text fragment joins across the element boundary; the widest segment
        // is the atom's own min-content (2), not the joined "a"+atom or
        // atom+"b" (which would be 3).
        let mut run = new_empty_run();
        accumulate_run(
            &mut run,
            text_member(1.0, 1.0, 1.0, 1.0, false, false, false),
            DomWhiteSpace::Normal,
        );
        accumulate_run(
            &mut run,
            element_atom_member(2.0, 2.0),
            DomWhiteSpace::Normal,
        );
        accumulate_run(
            &mut run,
            text_member(1.0, 1.0, 1.0, 1.0, false, false, false),
            DomWhiteSpace::Normal,
        );

        assert_eq!(run.widest_segment_px, 2.0);
        assert_eq!(run.max_content_sum_px, 4.0);
    }

    #[test]
    fn strut_is_the_max_member_line_height() {
        let mut run = new_empty_run();
        let mut tall = text_member(1.0, 1.0, 1.0, 1.0, false, false, false);
        tall.line_height_px = 22.0;
        accumulate_run(
            &mut run,
            text_member(1.0, 1.0, 1.0, 1.0, false, false, false),
            DomWhiteSpace::Normal,
        );
        accumulate_run(&mut run, tall, DomWhiteSpace::Normal);

        assert_eq!(run.strut_line_height_px, 22.0);
    }
}
