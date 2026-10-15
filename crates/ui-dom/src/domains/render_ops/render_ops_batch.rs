//! `RenderOpsBatch` is the owned payload moved out of a `RenderOpsHandler` at
//! drain time.
//!
//! This file owns the batch shape only: the staged applies and staged external
//! uploads present at the moment of a drain. It is the manual-move result for
//! hosts and tests without a render app, and the unit the extract moves into
//! the executor.
//!
//! Strict invariants:
//! - deferred best-effort work is promoted only after a drain, so it is never
//!   part of the batch that just left; this batch reflects exactly what was
//!   staged when `take_staged` ran.
//! - no revision/sequence/time fields may be added.

use super::{StagedExternalRgbaUpload, StagedLayerApply};

/// The owned payload moved out of a `RenderOpsHandler` at drain time; deferred
/// best-effort work is promoted only after a drain, so it is never part of the
/// batch that just left.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenderOpsBatch {
    /// Staged layer repaints, in stage order.
    pub applies: Vec<StagedLayerApply>,
    /// Staged external RGBA uploads, in stage order.
    pub external_uploads: Vec<StagedExternalRgbaUpload>,
}
