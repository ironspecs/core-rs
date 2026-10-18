#![allow(dead_code)]

//! Shared context types for coordination across multiple `RootDom`s.
//!
//! This file represents the shared runtime context every `RootDom` receives at
//! creation. Its purpose is to keep backend selection and shared style runtime
//! policy explicit instead of hiding them in paint globals or per-root one-off
//! state. It also carries the shared render-ops staging handler that owns one
//! tick's outgoing paint ops until the render-world handoff. The paint policy
//! (`PaintConfig`), pixel density (`PixelDensity`), and context configuration
//! (`RootDomContextConfig`) live in their own files and are re-exported here.

use std::sync::{Arc, Mutex};

pub use crate::types::paint_config::PaintConfig;
pub use crate::types::pixel_density::PixelDensity;
pub use crate::types::root_dom_context_config::RootDomContextConfig;

use crate::domains::style::{StyleRuleOrigin, StyleRuntime, StylesheetLoadReport};
use crate::types::{DomError, DomPseudoState};

/// Selects which paint executor owns layer rendering for one shared context.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaintBackend {
    /// Rasterize paint ops on CPU into `Image.data`.
    Cpu,
    /// Paint into the owned Bevy layer texture through the retained GPU compositor.
    Gpu,
}

/// Shared cross-root runtime context for `ui-dom`.
#[derive(Debug)]
pub struct RootDomContext {
    config: RootDomContextConfig,
    pub(crate) style_runtime: Mutex<StyleRuntime>,
    render_ops: Arc<crate::domains::RenderOpsHandler>,
}

impl RootDomContext {
    #[must_use]
    pub fn new(config: RootDomContextConfig) -> Self {
        Self::new_with_render_ops(config, Arc::new(crate::domains::RenderOpsHandler::new()))
    }

    /// Creates one context with a caller-provided shared render-ops handler.
    ///
    /// Contexts can deliberately share one handler (so several roots stage into
    /// the same outgoing-ops owner), and external producers can hold one with no
    /// `RootDom` at all.
    #[must_use]
    pub fn new_with_render_ops(
        config: RootDomContextConfig,
        render_ops: Arc<crate::domains::RenderOpsHandler>,
    ) -> Self {
        Self {
            config,
            style_runtime: Mutex::new(StyleRuntime::default()),
            render_ops,
        }
    }

    /// Creates a shared CPU context with backend policy only.
    #[must_use]
    pub fn new_cpu() -> Arc<Self> {
        Arc::new(Self::new(RootDomContextConfig::cpu()))
    }

    /// Creates a shared GPU context with backend policy only.
    #[must_use]
    pub fn new_gpu() -> Arc<Self> {
        Arc::new(Self::new(RootDomContextConfig::gpu()))
    }

    #[must_use]
    pub fn config(&self) -> RootDomContextConfig {
        self.config
    }

    #[must_use]
    pub fn paint_config(&self) -> PaintConfig {
        self.config.paint
    }

    #[must_use]
    pub fn debug_capture_enabled(&self) -> bool {
        self.config.debug_capture_enabled
    }

    /// Returns the shared render-ops staging handler this context uses for its
    /// outgoing paint ops.
    #[must_use]
    pub fn render_ops(&self) -> &crate::domains::RenderOpsHandler {
        &self.render_ops
    }

    /// Returns a clone of the shared handler `Arc`, used by attach registration
    /// to publish this context's handler to the render-world extract drain.
    pub(crate) fn render_ops_shared(&self) -> Arc<crate::domains::RenderOpsHandler> {
        Arc::clone(&self.render_ops)
    }

    /// Loads one stylesheet origin into the retained style runtime.
    ///
    /// Supported selectors include `#id`, `.class`, multi-class simple
    /// selectors like `.class_a.class_b` or `tag.class_a.class_b`, descendant
    /// whitespace between those selector steps, and the pseudo states
    /// `:hover`, `:active`, `:focus`, and `:focus-visible`.
    ///
    /// Dropped declarations are counted and reported, never silent (CSS 2.1
    /// §4.1.7): the returned `StylesheetLoadReport` carries applied/dropped
    /// declaration counts per load, and the context's
    /// [`RootDomContext::declaration_drop_count`] accumulates across loads.
    pub fn load_stylesheet(
        &self,
        origin: StyleRuleOrigin,
        css_text: &str,
    ) -> Result<StylesheetLoadReport, DomError> {
        let mut runtime = self
            .style_runtime
            .lock()
            .expect("style runtime mutex should not poison");
        runtime.load_stylesheet(origin, css_text)
    }

    /// Monotonic count of declarations dropped by drop-and-continue parsing
    /// across all loads on this context.
    #[must_use]
    pub fn declaration_drop_count(&self) -> u64 {
        let runtime = self
            .style_runtime
            .lock()
            .expect("style runtime mutex should not poison");
        runtime.declarations_dropped_count()
    }

    /// Live declaration blocks in the stylesheet arena (a level accessor over
    /// retained state, per the crate's observability doctrine).
    #[must_use]
    pub fn declaration_block_count(&self) -> usize {
        let runtime = self
            .style_runtime
            .lock()
            .expect("style runtime mutex should not poison");
        runtime.decl_block_count()
    }

    /// Unloads every active rule loaded under one stylesheet origin key.
    #[must_use]
    pub fn unload_stylesheet(&self, origin_key: &str) -> usize {
        let mut runtime = self
            .style_runtime
            .lock()
            .expect("style runtime mutex should not poison");
        runtime.unload_stylesheet(origin_key)
    }

    #[must_use]
    pub fn uses_pseudo_state_styles(&self, pseudo_state: DomPseudoState) -> bool {
        self.style_runtime
            .lock()
            .expect("style runtime mutex should not poison")
            .uses_pseudo_state(pseudo_state)
    }

    #[must_use]
    pub(crate) fn style_runtime_revision(&self) -> u64 {
        let runtime = self
            .style_runtime
            .lock()
            .expect("style runtime mutex should not poison");
        runtime.revision()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum DomDebugEvent {
    RootSync,
    RootHandleEvent,
    DeriveUi,
    DeriveUiChanged,
    DeriveUiUnchanged,
    DeriveLayout,
    DeriveLayoutChanged,
    DeriveLayoutUnchanged,
    DerivePaint,
    DerivePaintChanged,
    DerivePaintUnchanged,
    DeriveComposite,
    DeriveCompositeChanged,
    DeriveCompositeUnchanged,
    RouteClick,
    RouteWheel,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DomDebugReport {
    pub(crate) counts: [u64; 16],
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_reports_config_and_tracks_stylesheet_revision() {
        let context = RootDomContext::new(RootDomContextConfig::gpu());
        assert_eq!(context.paint_config().backend, PaintBackend::Gpu);
        assert!(!context.debug_capture_enabled());

        assert!(!context.uses_pseudo_state_styles(DomPseudoState::Hover));
        assert!(!context.uses_pseudo_state_styles(DomPseudoState::Active));
        let revision_before = context.style_runtime_revision();

        context
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 0),
                ".chip:hover { width: 10px; }",
            )
            .expect("hover stylesheet should load");

        assert!(context.uses_pseudo_state_styles(DomPseudoState::Hover));
        assert!(!context.uses_pseudo_state_styles(DomPseudoState::Active));
        assert!(context.style_runtime_revision() > revision_before);

        let revision_after_load = context.style_runtime_revision();
        let unloaded = context.unload_stylesheet("theme");
        assert_eq!(unloaded, 1);
        assert!(context.style_runtime_revision() > revision_after_load);
        assert!(!context.uses_pseudo_state_styles(DomPseudoState::Hover));
    }
}
