#![allow(dead_code)]

//! Shared context types for coordination across multiple `RootDom`s.
//!
//! This file represents the shared runtime context every `RootDom` receives at
//! creation. Its purpose is to keep backend selection and shared style runtime
//! policy explicit instead of hiding them in paint globals or per-root one-off
//! state.

use std::sync::{Arc, Mutex};

use crate::domains::style::{StyleRuleOrigin, StyleRuntime};
use crate::types::{DomError, DomPseudoState};

/// Selects which paint executor owns layer rendering for one shared context.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaintBackend {
    /// Rasterize paint ops on CPU into `Image.data`.
    Cpu,
    /// Paint into the owned Bevy layer texture through the retained GPU compositor.
    Gpu,
}

/// Selects the pixel density for owned layer textures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PixelDensity {
    /// One texture pixel per authored pixel.
    X1 = 1,
    /// Two texture pixels per authored pixel.
    X2 = 2,
    /// Four texture pixels per authored pixel.
    X4 = 4,
}

impl PixelDensity {
    #[must_use]
    pub fn multiplier(self) -> usize {
        match self {
            Self::X1 => 1,
            Self::X2 => 2,
            Self::X4 => 4,
        }
    }
}

/// Immutable paint policy shared by every `RootDom` using one context.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PaintConfig {
    /// Preferred execution target for retained layer paint.
    pub backend: PaintBackend,
}

impl PaintConfig {
    #[must_use]
    pub const fn new(backend: PaintBackend) -> Self {
        Self { backend }
    }
}

/// Configures one shared `RootDomContext`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RootDomContextConfig {
    /// Shared retained paint policy for all roots using this context.
    pub paint: PaintConfig,
    /// Enables controlled debug-only output capture.
    pub debug_capture_enabled: bool,
}

impl RootDomContextConfig {
    #[must_use]
    pub const fn new(paint: PaintConfig, debug_capture_enabled: bool) -> Self {
        Self {
            paint,
            debug_capture_enabled,
        }
    }

    /// Builds a CPU paint context config with no DOM-owned density policy.
    #[must_use]
    pub const fn cpu() -> Self {
        Self::new(PaintConfig::new(PaintBackend::Cpu), false)
    }

    /// Builds a GPU paint context config with no DOM-owned density policy.
    #[must_use]
    pub const fn gpu() -> Self {
        Self::new(PaintConfig::new(PaintBackend::Gpu), false)
    }
}

/// Shared cross-root runtime context for `ui-dom`.
#[derive(Debug)]
pub struct RootDomContext {
    config: RootDomContextConfig,
    pub(crate) style_runtime: Mutex<StyleRuntime>,
}

impl RootDomContext {
    #[must_use]
    pub fn new(config: RootDomContextConfig) -> Self {
        Self {
            config,
            style_runtime: Mutex::new(StyleRuntime::default()),
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

    /// Loads one stylesheet origin into the retained style runtime.
    ///
    /// Supported selectors include `#id`, `.class`, multi-class simple
    /// selectors like `.class_a.class_b` or `tag.class_a.class_b`, descendant
    /// whitespace between those selector steps, and the pseudo states
    /// `:hover`, `:active`, `:focus`, and `:focus-visible`.
    pub fn load_stylesheet(
        &self,
        origin: StyleRuleOrigin,
        css_text: &str,
    ) -> Result<usize, DomError> {
        let mut runtime = self
            .style_runtime
            .lock()
            .expect("style runtime mutex should not poison");
        runtime.load_stylesheet(origin, css_text)
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
