//! `LayerPoolRenderFeedback` is the render-to-main metric feedback bundle for retained layer work.
//! This file exists so render-world metric accumulation can stay temporary and explicit instead of mutating main-world counters directly from render code.
//!
//! The struct is intentionally tiny because it carries only the cross-world counts that need to be folded back into `LayerPool`.
//! Keeping the fold-back methods here makes the cross-world feedback contract auditable without reopening the executor implementation.
//!
//! Strict invariants:
//! - feedback is aggregate count data only, not render ownership state.
//! - render code accumulates into this bundle and main-world code applies it later.
//! - cross-world metric updates must stay explicit and bounded.

#[allow(clippy::wildcard_imports)]
use super::*;

#[derive(Default)]
pub(super) struct LayerPoolRenderFeedback {
    pub(super) shared_render_state_created: u64,
    pub(super) shared_rect_buffer_grew: u64,
    pub(super) repaint_applied: u64,
    pub(super) rect_gpu_work_applied: u64,
}

impl LayerPoolRenderFeedback {
    pub(super) fn is_empty(&self) -> bool {
        self.shared_render_state_created == 0
            && self.shared_rect_buffer_grew == 0
            && self.repaint_applied == 0
            && self.rect_gpu_work_applied == 0
    }

    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }

    pub(super) fn apply_to(&self, layer_pool: &mut LayerPool) {
        increment_metric_count(
            &mut layer_pool.metrics,
            LayerPoolMetricName::SharedRenderStateCreated,
            self.shared_render_state_created,
        );
        increment_metric_count(
            &mut layer_pool.metrics,
            LayerPoolMetricName::SharedRectBufferGrew,
            self.shared_rect_buffer_grew,
        );
        increment_metric_count(
            &mut layer_pool.metrics,
            LayerPoolMetricName::RepaintApplied,
            self.repaint_applied,
        );
        increment_metric_count(
            &mut layer_pool.metrics,
            LayerPoolMetricName::RectGpuWorkApplied,
            self.rect_gpu_work_applied,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feedback_empties_applies_and_clears() {
        let mut feedback = LayerPoolRenderFeedback::default();
        assert!(feedback.is_empty());

        feedback.shared_render_state_created = 3;
        assert!(!feedback.is_empty());

        let mut pool = LayerPool::new(LayerConfig::default());
        let baseline = pool.count(LayerPoolMetricName::SharedRenderStateCreated);
        assert_eq!(baseline, 0);

        feedback.apply_to(&mut pool);
        assert_eq!(
            pool.count(LayerPoolMetricName::SharedRenderStateCreated),
            baseline + 3
        );

        feedback.clear();
        assert!(feedback.is_empty());
    }
}
