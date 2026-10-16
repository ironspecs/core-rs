//! Shared root context configuration for `ui-dom`.
//!
//! This file represents the configuration every `RootDom` receives at
//! creation. Its purpose is to keep backend selection and debug-output policy
//! explicit instead of hiding them in paint globals or per-root one-off state.
//!
//! Invariants and rules:
//!
//! - this file owns only the `RootDomContextConfig` struct and its `impl` blocks
//! - the config is immutable and copyable; it carries no runtime state

use crate::types::paint_config::PaintConfig;
use crate::types::root_dom_context::PaintBackend;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_config_presets_carry_expected_paint_and_debug_flags() {
        let cpu = RootDomContextConfig::cpu();
        assert_eq!(cpu.paint.backend, PaintBackend::Cpu);
        assert!(!cpu.debug_capture_enabled);

        let gpu = RootDomContextConfig::gpu();
        assert_eq!(gpu.paint.backend, PaintBackend::Gpu);
        assert!(!gpu.debug_capture_enabled);

        let configured = RootDomContextConfig::new(PaintConfig::new(PaintBackend::Gpu), true);
        assert!(configured.debug_capture_enabled);
    }
}
