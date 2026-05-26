//! Shared runtime domains for `ui-dom`.
//!
//! This file represents the domain-level boundary that groups the retained
//! layout, minimal paint, hit-test, composite, and retained-layer policy used
//! by `ui-dom`. Its purpose is to keep cross-cutting runtime logic organized
//! without leaking app-local ownership or scene policy into the crate boundary.
//!
//! Invariants and rules:
//!
//! - this file only wires domain modules together
//! - domain ownership should remain explicit in the submodules themselves
//! - no second public boundary should be hidden here behind convenience exports
//!
//! Advice and code philosophy:
//!
//! - keep domain grouping simple and descriptive
//! - surface shared runtime seams, not migration clutter
pub mod composite;
pub(crate) mod control_widgets;
pub(crate) mod hit_test;
pub(crate) mod icon_atlas;
pub mod keyboard;
pub(crate) mod layer_hit;
pub(crate) mod layer_ops;
pub mod layer_pool;
pub mod layout;
pub(crate) mod paint;
pub mod style;
pub(crate) mod text_layout;
pub(crate) mod visitor;

pub use crate::types::{
    ROOT_DOM_METRIC_HISTORY_CAPACITY, RootDomMetricHistoryEntry, RootDomMetricHistoryIter,
    RootDomMetricName, RootDomMetrics,
};
pub use keyboard::dispatch_bevy_keyboard_input_from_focus_and_apply_default_behavior;
pub use layer_pool::{
    ExternalLayerTarget, LayerConfig, LayerLease, LayerPool, LayerPoolApplyError,
    LayerPoolMetricHistoryEntry, LayerPoolMetricHistoryIter, LayerPoolMetricName, LayerPoolMetrics,
    LayerPoolPolicy,
};
pub(crate) use layer_pool::{
    LayerPoolRenderExecutor, LayerPoolRenderJobQueue, apply_layer_pool_render_work,
    extract_layer_pool_render_work,
};
pub use layout::{
    DomLayoutInput, DomLayoutResolver, IdentityDomLayoutResolver, node_box, node_layout_dirty,
    node_paint_dirty, root_key,
};
pub use style::{ComputedStyleMap, StyleRule, StyleRuleKey, StyleRuleOrigin};
