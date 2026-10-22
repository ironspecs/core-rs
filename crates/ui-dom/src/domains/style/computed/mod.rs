//! Computed-value store for the `ui-dom` style system (spine step 3.1).
//!
//! This module owns the seven inheritance-grouped computed groups from
//! `PLAN.md` proposal S1.1: font, text, and color (inherited) plus box, flex,
//! visual, and transform (non-inherited) — the physical "presentation half" of
//! a node's computed style that will be inlined into the node record beside
//! `box_model`, `layout`, and `paint`. It also owns the containers and helpers
//! that sit around those groups: `ComputedGroups` (the seven held together),
//! `NodePresentation` (inline declaration block key + computed groups +
//! transition longhands + the last-pass `previous_computed` baseline),
//! `ResolvedTransitionLonghands` (the scheduler-facing slot),
//! `GroupDirtyClassification` (A7's static group-diff table), and
//! `for_each_moved_field` (the field-level compare the transition scheduler
//! keys on).
//!
//! Invariants and rules:
//!
//! - **Wired in spine step 3.3a; the winners fold writes the groups since
//!   3.3g-2a.** The style pass writes the seven groups via the typed winners
//!   fold (`domains/visitor/winners_fold.rs`, through `project.rs`'s shared
//!   resolver); the NEW view accessors (`ComputedStyleRef`) read them. The
//!   transition-overlay merge runs inside the `DomNode::computed_*` accessors,
//!   which `ComputedStyleRef::as_*` delegates to.
//! - **`mod.rs` holds no behavior-bearing types** (structural rule 1): it only
//!   declares the submodules and re-exports the public read surface.
//! - **Groups are plain `Copy` data.** Each group is a small `Copy` struct
//!   with `initial()` plus tiny field helpers only. Resolution logic (A5
//!   defaulting/inheritance, A6 unit resolution) lives in `project.rs`, the
//!   step-3.3 resolver.
//! - **Inherited vs non-inherited seeding is CSS defaulting.** The three
//!   inherited groups (font/text/color) seed from the parent's group; the four
//!   non-inherited groups seed from their own `initial()` (A5, CSS Cascade §7).

mod align_content_kw;
mod align_items_kw;
mod box_length;
mod box_sizing_kw;
mod computed_box_group;
mod computed_color_group;
mod computed_flex_group;
mod computed_font_group;
mod computed_grid_group;
mod computed_groups;
mod computed_style_ref;
mod computed_text_group;
mod computed_transform_group;
mod computed_visual_group;
mod display_kw;
mod field_mapping;
mod flex_basis_kw;
mod flex_direction_kw;
mod flex_wrap_kw;
mod font_family_generic;
mod grid_auto_flow_kw;
mod grid_line;
mod grid_template_areas;
mod grid_track_list;
mod grid_track_size;
mod group_dirty_classification;
mod justify_content_kw;
mod justify_items_kw;
mod justify_self_kw;
mod node_presentation;
mod overflow_kw;
mod position_kw;
pub(crate) mod project;
mod resolved_transition_longhands;
mod text_family_list;

pub use align_content_kw::AlignContentKw;
pub use align_items_kw::AlignItemsKw;
pub use box_length::BoxLength;
pub use box_sizing_kw::BoxSizingKw;
pub use computed_box_group::ComputedBoxGroup;
pub use computed_color_group::ComputedColorGroup;
pub use computed_flex_group::ComputedFlexGroup;
pub use computed_font_group::ComputedFontGroup;
pub use computed_grid_group::ComputedGridGroup;
pub use computed_groups::ComputedGroups;
pub use computed_style_ref::ComputedStyleRef;
pub use computed_text_group::ComputedTextGroup;
pub use computed_transform_group::ComputedTransformGroup;
pub use computed_visual_group::ComputedVisualGroup;
pub use display_kw::DisplayKw;
pub use flex_basis_kw::FlexBasisKw;
pub use flex_direction_kw::FlexDirectionKw;
pub use flex_wrap_kw::FlexWrapKw;
pub use font_family_generic::FontFamilyGeneric;
pub use grid_auto_flow_kw::GridAutoFlowKw;
pub use grid_line::GridLine;
pub(crate) use grid_template_areas::{
    AreaRect, GRID_AREA_COL_CAPACITY, GRID_AREA_NAME_CAPACITY, GRID_AREA_ROW_CAPACITY,
    GridTemplateAreas,
};
pub(crate) use grid_track_list::{
    GRID_LINE_NAME_CAPACITY, GRID_TRACK_LIST_CAPACITY, GridRepeat, GridTrackList,
};
pub use grid_track_size::{GridTrackSize, TrackBreadth, TrackMaxBreadth};
pub use group_dirty_classification::GroupDirtyClassification;
pub use justify_content_kw::JustifyContentKw;
pub use justify_items_kw::JustifyItemsKw;
pub use justify_self_kw::JustifySelfKw;
pub use node_presentation::NodePresentation;
pub use overflow_kw::OverflowKw;
pub use position_kw::PositionKw;
pub use resolved_transition_longhands::{
    ResolvedTransitionLonghands, TRANSITION_LONGHAND_CAPACITY, TransitionLonghandProperty,
};
pub use text_family_list::{TextFamilyItem, TextFamilyList};

pub(crate) use field_mapping::for_each_moved_field;
