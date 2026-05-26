//! `LayerPoolMetricName` names the fixed retained-layer metrics exposed by `LayerPool`.
//! This file exists so metric identity stays stable and explicit instead of drifting into stringly typed counters or ad hoc slot folklore.
//!
//! The enum is separated from the metric store so the metric vocabulary can be reviewed as a boundary on its own.
//! The fixed index mapping is kept local to this file because the metric layout is part of the retained-layer contract, not a caller concern.
//!
//! Strict invariants:
//! - metric names are a closed set, not dynamic strings.
//! - each metric name maps to one stable slot index.
//! - callers should use these names directly instead of depending on slot order.

#[allow(clippy::wildcard_imports)]
use super::*;

/// Names one `LayerPool` metric counter inside the crate.
///
/// Keep this as the direct metric parameter type for internal reads. Do not
/// replace it with strings, slot indices, or conversion helpers.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum LayerPoolMetricName {
    /// Number of new logical layer leases created.
    LayerCreated,
    /// Number of lease updates that changed retained mapping fields.
    LayerMappingUpdated,
    /// Number of lease updates that moved a lease onto a larger retained surface.
    SurfaceGrew,
    /// Number of fresh retained surfaces allocated because no free exact-size match existed.
    SurfaceAllocated,
    /// Number of retained surfaces rebound from the free exact-size pool.
    SurfaceReused,
    /// Number of retained surfaces returned to the free exact-size pool.
    SurfaceReleased,
    /// Number of logical layer leases destroyed.
    LayerDestroyed,
    /// Number of stale lease attempts rejected before any retained mutation.
    StaleLeaseRejected,
    /// Number of policy updates accepted for existing layer leases.
    PolicyChanged,
    /// Number of best-effort retained uploads deferred under queue pressure.
    UploadDeferred,
    /// Number of offscreen retained uploads suppressed before queueing.
    UploadSuppressed,
    /// Number of DOM-driven invalidations bypassed for external layers.
    InvalidationBypassed,
    /// Number of shared render-state initializations performed by the pool.
    SharedRenderStateCreated,
    /// Number of shared rect GPU buffer growth events.
    SharedRectBufferGrew,
    /// Number of repaint applications that performed retained GPU work.
    RepaintApplied,
    /// Number of rect-family GPU executions performed by the pool.
    RectGpuWorkApplied,
}

pub(super) const LAYER_POOL_METRIC_NAMES: [LayerPoolMetricName; 16] = [
    LayerPoolMetricName::LayerCreated,
    LayerPoolMetricName::LayerMappingUpdated,
    LayerPoolMetricName::SurfaceGrew,
    LayerPoolMetricName::SurfaceAllocated,
    LayerPoolMetricName::SurfaceReused,
    LayerPoolMetricName::SurfaceReleased,
    LayerPoolMetricName::LayerDestroyed,
    LayerPoolMetricName::StaleLeaseRejected,
    LayerPoolMetricName::PolicyChanged,
    LayerPoolMetricName::UploadDeferred,
    LayerPoolMetricName::UploadSuppressed,
    LayerPoolMetricName::InvalidationBypassed,
    LayerPoolMetricName::SharedRenderStateCreated,
    LayerPoolMetricName::SharedRectBufferGrew,
    LayerPoolMetricName::RepaintApplied,
    LayerPoolMetricName::RectGpuWorkApplied,
];

impl LayerPoolMetricName {
    /// Number of defined `LayerPoolMetricName` variants.
    pub const COUNT: usize = LAYER_POOL_METRIC_NAMES.len();

    const fn index(self) -> usize {
        match self {
            Self::LayerCreated => 0,
            Self::LayerMappingUpdated => 1,
            Self::SurfaceGrew => 2,
            Self::SurfaceAllocated => 3,
            Self::SurfaceReused => 4,
            Self::SurfaceReleased => 5,
            Self::LayerDestroyed => 6,
            Self::StaleLeaseRejected => 7,
            Self::PolicyChanged => 8,
            Self::UploadDeferred => 9,
            Self::UploadSuppressed => 10,
            Self::InvalidationBypassed => 11,
            Self::SharedRenderStateCreated => 12,
            Self::SharedRectBufferGrew => 13,
            Self::RepaintApplied => 14,
            Self::RectGpuWorkApplied => 15,
        }
    }
}

impl MetricName for LayerPoolMetricName {
    fn index(self) -> usize {
        LayerPoolMetricName::index(self)
    }
}
