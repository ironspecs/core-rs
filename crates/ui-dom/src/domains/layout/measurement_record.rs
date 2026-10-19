//! Per-node intrinsic measurement record for the shared layout up fold.
//!
//! This file owns `MeasurementRecord`, the shared exit-time computation
//! `compute_node_measurement`, and the equality-gated write gate
//! `replace_measurement_if_changed`. Its purpose is to give each node one
//! retained slot that the layout pass writes exactly once per dirty pass (the
//! block world at `exit_node`, the flex subtree at the measure fold), carrying
//! the subtree-computed content sizes the flex solver reads in the same pass.
//!
//! Invariants and rules:
//!
//! - only basis-independent intrinsics live in the `h_*`/`v_*` content slots;
//!   `content_height_at_width_px` is this-pass measured and therefore
//!   basis-dependent by design (it is the answer placement needs now)
//! - the `v_*` slots stay `None` until their named consumer lands in flex
//!   step 4; nothing computes what nothing reads
//! - record writes are equality-gated through `replace_measurement_if_changed`
//!   and are a pass OUTPUT: they never mark style/layout/paint dirty

use crate::domains::layout::measurement_flags::MeasurementFlags;
use crate::types::DomWhiteSpace;

use super::flow::EnteredNodeLayout;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MeasurementRecord {
    /// Min-content inline size, computed bottom-up (the block fold's
    /// `min_width_px`).
    pub h_min_content_px: f32,
    /// Max-content inline size, computed bottom-up (the block fold's
    /// `max_width_px`).
    pub h_max_content_px: f32,
    /// Content height at this pass's laid-out width.
    pub content_height_at_width_px: f32,
    /// Min-content block size. Uncomputed in step 1; lands with its named
    /// consumer in flex step 4.
    pub v_min_content_px: Option<f32>,
    /// Max-content block size. Uncomputed in step 1; lands with its named
    /// consumer in flex step 4.
    pub v_max_content_px: Option<f32>,
    /// Fixed measurement class flags; see `MeasurementFlags`.
    pub flags: MeasurementFlags,
}

impl MeasurementRecord {
    /// Equality-gated record write: stores `next` only when it differs from
    /// the current value, returning whether a write occurred. This is the
    /// write gate the layout up fold uses at node exit; a write is a pass
    /// output and never marks style/layout/paint dirty.
    pub fn replace_measurement_if_changed(&mut self, next: MeasurementRecord) -> bool {
        if *self == next {
            return false;
        }
        *self = next;
        true
    }
}

/// Computes the exit-time measurement record for one node from the shared
/// block/inline/flex exit epilogue's data.
///
/// `min_content_width_px`/`max_content_width_px` are the CONTENT inline-size
/// contributions from `content_width_contribution_for_exit` (before this
/// node's own horizontal border+padding — the value `layout_flow_size` adds
/// edges to). `content_height_at_width_px` is the resolved content height at
/// this pass's laid-out width (the `ResolvedContentSizePx::viewport_height_px`
/// the box model derives from). The `v_*` slots stay `None`: their named
/// consumer (the step-4 auto-min walk) has not landed, so nothing is computed.
///
/// Flags are the two fixed measurement class bits read from the entered node:
/// `PRESERVE_MODE` from the node's effective white-space class (preserve =
/// `pre`/`pre-wrap`/`break-spaces`; collapse = everything else), and
/// `REPLACED_RATIO` from an img's intrinsic aspect ratio. The effective
/// white-space is read from `entered.inherited_style`, which for an element is
/// its own computed text group and for a text node is the inherited group —
/// the value that actually applies at the node.
pub(super) fn compute_node_measurement(
    entered: &EnteredNodeLayout,
    min_content_width_px: f32,
    max_content_width_px: f32,
    content_height_at_width_px: f32,
) -> MeasurementRecord {
    let mut flags = MeasurementFlags::empty();
    if white_space_is_preserve_mode(entered.inherited_style.effective_white_space()) {
        flags = flags.with_preserve_mode();
    }
    if entered.img_intrinsic_aspect_ratio.is_some() {
        flags = flags.with_replaced_ratio();
    }
    MeasurementRecord {
        h_min_content_px: min_content_width_px,
        h_max_content_px: max_content_width_px,
        content_height_at_width_px,
        v_min_content_px: None,
        v_max_content_px: None,
        flags,
    }
}

/// The preserve white-space class (`pre`/`pre-wrap`/`break-spaces`) carries
/// literal space runs; every other class (`normal`, `nowrap`, `pre-line`)
/// collapses them. This mirrors the crate's `text_layout_input_for_white_space`
/// grouping so the measurement class flag matches the text shaping path.
fn white_space_is_preserve_mode(white_space: DomWhiteSpace) -> bool {
    matches!(
        white_space,
        DomWhiteSpace::Pre | DomWhiteSpace::PreWrap | DomWhiteSpace::BreakSpaces
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_record_has_none_v_slots_and_zero_flags() {
        let record = MeasurementRecord::default();

        // The derived Default keeps every slot zero and the v slots uncomputed.
        assert_eq!(
            record,
            MeasurementRecord {
                h_min_content_px: 0.0,
                h_max_content_px: 0.0,
                content_height_at_width_px: 0.0,
                v_min_content_px: None,
                v_max_content_px: None,
                flags: MeasurementFlags { bits: 0 },
            }
        );
        assert_eq!(record.v_min_content_px, None);
        assert_eq!(record.v_max_content_px, None);
        assert_eq!(record.flags.bits, 0);
    }

    #[test]
    fn replace_measurement_if_changed_rejects_equal_write() {
        let mut record = MeasurementRecord {
            h_min_content_px: 10.0,
            h_max_content_px: 20.0,
            content_height_at_width_px: 15.0,
            v_min_content_px: None,
            v_max_content_px: None,
            flags: MeasurementFlags { bits: 1 },
        };
        let equal = record;

        assert!(!record.replace_measurement_if_changed(equal));
        assert_eq!(record, equal);
    }

    #[test]
    fn replace_measurement_if_changed_writes_changed_value() {
        let mut record = MeasurementRecord::default();
        let next = MeasurementRecord {
            h_min_content_px: 10.0,
            h_max_content_px: 20.0,
            content_height_at_width_px: 15.0,
            v_min_content_px: None,
            v_max_content_px: None,
            flags: MeasurementFlags { bits: 2 },
        };

        assert!(record.replace_measurement_if_changed(next));
        assert_eq!(record, next);
    }

    #[test]
    fn replace_measurement_if_changed_treats_none_to_some_v_slot_as_change() {
        let mut record = MeasurementRecord::default();
        let next = MeasurementRecord {
            v_min_content_px: Some(8.0),
            ..MeasurementRecord::default()
        };

        assert!(record.replace_measurement_if_changed(next));
        assert_eq!(record.v_min_content_px, Some(8.0));
        assert_eq!(record.v_max_content_px, None);
        assert_eq!(record.flags.bits, 0);
    }

    #[test]
    fn white_space_preserve_class_matches_text_shaping_grouping() {
        assert!(white_space_is_preserve_mode(DomWhiteSpace::Pre));
        assert!(white_space_is_preserve_mode(DomWhiteSpace::PreWrap));
        assert!(white_space_is_preserve_mode(DomWhiteSpace::BreakSpaces));
        assert!(!white_space_is_preserve_mode(DomWhiteSpace::Normal));
        assert!(!white_space_is_preserve_mode(DomWhiteSpace::Nowrap));
        assert!(!white_space_is_preserve_mode(DomWhiteSpace::PreLine));
    }
}
