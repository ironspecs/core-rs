//! Per-node intrinsic measurement record for the shared layout up fold.
//!
//! This file owns `MeasurementRecord` and its equality-gated write gate
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
}
