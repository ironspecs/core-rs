//! Flex solver — orchestration, shared vocabulary, and types.
//!
//! This file owns the flex solver's entry and phase driving, the lane/context
//! declarations and the style/axis/margin vocabulary they share, and the
//! re-exports that keep the shared measure/placement helpers reachable from the
//! grid solver. The solver's actual phases live in four sibling files, in the
//! order they run:
//!
//! - `flex_formation.rs` — item FORMATION (anonymous runs, blockification,
//!   abs-pos exclusion, order-lane construction, item-lane writes)
//! - `flex_measure.rs` — the measurement fold (item measurement, item-view
//!   assembly, measurement-record interplay, intrinsic contributions)
//! - `flex_resolve.rs` — §9.7 sizing (line building, the freeze loop,
//!   base/hypothetical sizing, flex factor distribution, resolution)
//! - `flex_place.rs` — final placement (order-modified placement, cross-axis
//!   alignment, auto-margin absorption, excluded-child static placement)
//!
//! Invariants and rules:
//!
//! - flex solving stays inside the shared walker and shared enter/exit pattern
//! - transient solve state lives only for the current solver call stack
//! - this file must not persist solver scratch on nodes or elsewhere
//! - temporary state here must be algorithmic necessity, not caching for its
//!   own sake
//! - final node layout writes use active flex placement with anonymous flex
//!   item generation for inline-level children per CSS Flexbox §4
#![allow(clippy::needless_pass_by_value)] // LayoutContext is passed by value as a child's owned context, mirroring flow.rs

use crate::domains::style::computed::{
    AlignContentKw, AlignItemsKw, BoxLength, FlexDirectionKw, FlexWrapKw, JustifyContentKw,
};
use crate::domains::visitor::inherited_style::InheritedStyleContext;
use crate::numeric::usize_to_f32;
use crate::types::{
    DomError, DomNode, DomNodeSlotKey, DomTextDirection, LaneRange, RootDom, RootDomMetricName,
};

use super::DomLayoutResolver;
use super::flex_item_source::FlexItemSource;
use super::flex_measure::build_staged_measurement_solve_from_container;
use super::flex_place::exit_active_flex_container;
use super::flex_resolve::resolve_active_flex_container;
use super::flex_wrap_mode::FlexWrapMode;
use super::flow::{
    EnteredNodeLayout, FlowSize, LayoutContext, child_layout_context, clamp_length, enter_node,
};
use super::style::{BoxSides, NodeStyle};

pub(super) use super::flex_formation::measure_inline_run;
pub(super) use super::flex_measure::{
    measure_tree_walker_node_without_layout_write, node_has_main_influences_cross_child,
};
pub(super) use super::flex_place::write_node_border_box_position;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FlexAxis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FlexMainAxisAlignment {
    Start,
    End,
    FlexStart,
    FlexEnd,
    Center,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FlexCrossAxisAlignment {
    Start,
    End,
    FlexStart,
    FlexEnd,
    Center,
    Stretch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FlexLineCrossAxisAlignment {
    Start,
    End,
    FlexStart,
    FlexEnd,
    Center,
    Stretch,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FlexGapSizes {
    pub(crate) between_items_px: f32,
    pub(crate) between_lines_px: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum FlexBasisSource {
    Auto,
    Content,
    Zero,
    Length(f32),
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FlexContainerStyle {
    pub(crate) main_axis: FlexAxis,
    pub(crate) is_main_reversed: bool,
    pub(crate) text_direction: DomTextDirection,
    pub(crate) wrap_mode: FlexWrapMode,
    pub(crate) justify_content: FlexMainAxisAlignment,
    pub(crate) align_items: FlexCrossAxisAlignment,
    pub(crate) align_content: FlexLineCrossAxisAlignment,
    pub(crate) gap_sizes: FlexGapSizes,
}

/// The flex item view the solve consumes in place of the former
/// 20-field `MeasuredFlexItem`. Its CONTENT-SIZE fields
/// (`flex_base_size_px` for the auto/content basis case,
/// `measured_cross_size_input_px`, and the derived
/// `hypothetical_main_size_px`/`outer_hypothetical_main_size_px`) are
/// derived from the child's measurement record (written by the measure fold
/// just before this view is assembled); every style-derived field keeps its
/// exact production site and resolution timing in `measure_one_flex_item`.
/// `main_axis`/`cross_axis` died here (dead in production: the container style
/// answers both axes).
///
/// `h_min_content_px`/`h_max_content_px` carry the item's record inline
/// min/max-content sizes (D2a, flex arc step 4): the inner container's §9.9
/// intrinsic main-size fold reads them at the container's record write. For a
/// run item (no node record) they carry the run's widest segment / max-content
/// sum, the 3a cursors.
#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(clippy::struct_excessive_bools)] // the §8.1 auto-margin flags are four named side bits (D1); a bitmask would obscure the per-side reads
pub(crate) struct FlexItemView {
    pub(crate) source: FlexItemSource,
    pub(crate) order: i32,
    pub(crate) align_self: Option<FlexCrossAxisAlignment>,
    pub(crate) flex_grow_factor: f32,
    pub(crate) flex_shrink_factor: f32,
    pub(crate) flex_base_size_px: f32,
    pub(crate) hypothetical_main_size_px: f32,
    pub(crate) automatic_min_main_size_px: f32,
    pub(crate) min_main_size_px: Option<f32>,
    pub(crate) max_main_size_px: Option<f32>,
    pub(crate) main_axis_border_padding_px: f32,
    pub(crate) outer_hypothetical_main_size_px: f32,
    pub(crate) measured_cross_size_input_px: f32,
    /// Whether this item is a replaced element with an intrinsic aspect ratio
    /// (`record.flags.replaced_ratio`). `stretch` must not distort it — the
    /// cross follows the ratio from the resolved main size, not the line cross.
    pub(crate) replaced_ratio: bool,
    /// Whether the item's subtree is flagged `main_influences_cross` — its
    /// cross size may change when its main size changes (text wraps, percentage
    /// cross forms). A flagged item whose resolved main differs from its
    /// hypothetical main refolds at item-cross-at-resolved-main (D1.2).
    pub(crate) main_influences_cross: bool,
    /// The item's resolved cross size (item cross at resolved main, flex arc
    /// step 7 D1.2). Seeded to the measured cross at assembly; the resolve
    /// phase overwrites it for replaced-ratio and flagged-and-width-changed
    /// (refolded) items before line cross sizing. Placement reads this for the
    /// cross-axis alignment offset and the stretch decision.
    pub(crate) resolved_cross_size_px: f32,
    pub(crate) h_min_content_px: f32,
    pub(crate) h_max_content_px: f32,
    pub(crate) margin_main_start_px: f32,
    pub(crate) margin_main_end_px: f32,
    pub(crate) margin_cross_start_px: f32,
    pub(crate) margin_cross_end_px: f32,
    /// CSS Flexbox §8.1 auto-margin flags (D1, flex arc step 10): one bit per
    /// axis side, read from the node's computed box group at item-view
    /// assembly (`BoxLength::Auto`). Auto margins are NOT resolved at
    /// measure/style time — they need free space, which exists only after §9.7
    /// — so the resolved `margin_*_px` fields carry a 0.0 placeholder here and
    /// the absorption phases (D2 main, D3 cross) overwrite them.
    pub(crate) margin_main_start_auto: bool,
    pub(crate) margin_main_end_auto: bool,
    pub(crate) margin_cross_start_auto: bool,
    pub(crate) margin_cross_end_auto: bool,
}

/// The transient resolved-item bundle placement assembles per child from the
/// lanes (view + resolution + packing offsets). It carries more than a pure
/// view+resolution join — `main_offset_px` and `cross_offset_px` are the
/// line/axis packing results the resolve phase computed — so it survives as a
/// small `Copy` bundle rather than being replaced by bare accessors.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ResolvedFlexItem {
    pub(crate) source: FlexItemSource,
    pub(crate) align_self: Option<FlexCrossAxisAlignment>,
    pub(crate) resolved_main_size_px: f32,
    pub(crate) main_axis_border_padding_px: f32,
    pub(crate) resolved_cross_size_px: f32,
    pub(crate) replaced_ratio: bool,
    pub(crate) main_offset_px: f32,
    pub(crate) cross_offset_px: f32,
}

/// The lane-based flex solve for one container: the four lane ranges plus the
/// container's resolved main/cross sizes. After the measurement fold the sizes
/// are the measured-container intrinsic sizes (widest line main size, summed
/// line cross size); `resolve_active_flex_container` overwrites them with the
/// resolved container sizes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FlexContainerSolve {
    pub(crate) item_range: LaneRange,
    pub(crate) order_range: LaneRange,
    pub(crate) line_range: LaneRange,
    pub(crate) resolution_range: LaneRange,
    pub(crate) main_size_px: f32,
    pub(crate) cross_size_px: f32,
}

/// The placement result: the container's resolved cross size. The former
/// `resolved_items` vector was dead after placement (written back into the
/// solve and never read), so it dies here.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PlacedFlexChildren {
    pub(crate) container_cross_size_px: f32,
}

pub(crate) struct StagedMeasurementRequest {
    pub(crate) key: DomNodeSlotKey,
    pub(crate) child_context: LayoutContext,
    pub(crate) container_style: FlexContainerStyle,
    pub(crate) container_main_size_px: Option<f32>,
}

pub(crate) struct ActiveFlexExitRequest {
    pub(crate) key: DomNodeSlotKey,
    pub(crate) context: LayoutContext,
    pub(crate) entered: EnteredNodeLayout,
    pub(crate) child_context: LayoutContext,
    pub(crate) container_style: FlexContainerStyle,
    pub(crate) solve: FlexContainerSolve,
}

pub(crate) struct ActiveFlexPlacementRequest<'a> {
    pub(crate) child_context: &'a LayoutContext,
    pub(crate) container_style: FlexContainerStyle,
    pub(crate) container_main_size_px: f32,
    pub(crate) solve: FlexContainerSolve,
}

pub(crate) struct ActiveFlexChildRelayoutRequest<'a> {
    pub(crate) source: FlexItemSource,
    pub(crate) child_context: &'a LayoutContext,
    pub(crate) container_style: FlexContainerStyle,
    pub(crate) container_main_size_px: f32,
    pub(crate) line_cross_size_px: f32,
    pub(crate) resolved_item: ResolvedFlexItem,
    /// The item view, carried into placement so the child context can place the
    /// item's content origin at the absorbed start margins BEFORE the subtree
    /// relayout (the auto-margin flags + absorbed px live here, not on the
    /// resolved-item bundle).
    pub(crate) view: FlexItemView,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct MeasuredBlockFlowState {
    pub(crate) block_top_px: f32,
    pub(crate) inline_left_px: f32,
    pub(crate) line_height_px: f32,
    pub(crate) max_width_px: f32,
    pub(crate) line_min_width_px: f32,
    pub(crate) line_max_width_px: f32,
    pub(crate) min_width_px: f32,
    pub(crate) max_content_width_px: f32,
    pub(crate) has_children: bool,
}

pub(super) fn relayout_flex_node<R>(
    root_dom: &mut RootDom,
    tree_size: (f32, f32),
    key: DomNodeSlotKey,
    context: LayoutContext,
    style: NodeStyle,
    inherited_style: InheritedStyleContext,
    resolver: &mut R,
) -> Result<FlowSize, DomError>
where
    R: DomLayoutResolver + ?Sized,
{
    relayout_active_flex_node(
        root_dom,
        tree_size,
        key,
        context,
        style,
        inherited_style,
        resolver,
    )
}

fn relayout_active_flex_node<R>(
    root_dom: &mut RootDom,
    tree_size: (f32, f32),
    key: DomNodeSlotKey,
    context: LayoutContext,
    style: NodeStyle,
    inherited_style: InheritedStyleContext,
    resolver: &mut R,
) -> Result<FlowSize, DomError>
where
    R: DomLayoutResolver + ?Sized,
{
    let container_node = node_for_key(root_dom, key)?;
    let container_style = staged_flex_container_style(container_node);
    let entered = enter_node(
        root_dom,
        tree_size,
        key,
        context.clone(),
        style,
        inherited_style,
        resolver,
    )?;
    let container_main_size_px = entered_container_main_size(&entered, container_style.main_axis);
    let child_context = child_layout_context(context.clone(), entered.clone());
    let container_cross_size_px = entered_container_cross_size(&entered, container_style.main_axis);
    let measured_solve = build_staged_measurement_solve_from_container(
        root_dom,
        tree_size,
        &StagedMeasurementRequest {
            key,
            child_context: child_context.clone(),
            container_style,
            container_main_size_px,
        },
        resolver,
        RootDomMetricName::FlexNodesMeasured,
    )?;
    let resolved_container_main_size_px =
        container_main_size_px.unwrap_or(measured_solve.main_size_px);
    let resolved_solve = resolve_active_flex_container(
        root_dom,
        tree_size,
        &child_context,
        measured_solve,
        container_style,
        resolved_container_main_size_px,
        container_cross_size_px,
        resolver,
    )?;
    exit_active_flex_container(
        root_dom,
        tree_size,
        ActiveFlexExitRequest {
            key,
            context,
            entered,
            child_context,
            container_style,
            solve: resolved_solve,
        },
        resolver,
    )
}

pub(crate) fn staged_flex_container_style(node: &DomNode) -> FlexContainerStyle {
    let flex = node.computed_flex();
    let (main_axis, is_main_reversed) = flex_direction_from_kw(flex.direction);
    FlexContainerStyle {
        main_axis,
        is_main_reversed,
        text_direction: effective_flex_container_text_direction(node),
        wrap_mode: flex_wrap_mode_from_kw(flex.wrap),
        justify_content: justify_content_from_kw(flex.justify_content),
        align_items: align_items_from_kw(flex.align_items),
        align_content: align_content_from_kw(flex.align_content),
        gap_sizes: flex_gap_sizes(flex.gap, main_axis),
    }
}

fn flex_direction_from_kw(kw: FlexDirectionKw) -> (FlexAxis, bool) {
    match kw {
        FlexDirectionKw::Row => (FlexAxis::Horizontal, false),
        FlexDirectionKw::RowReverse => (FlexAxis::Horizontal, true),
        FlexDirectionKw::Column => (FlexAxis::Vertical, false),
        FlexDirectionKw::ColumnReverse => (FlexAxis::Vertical, true),
    }
}

fn flex_wrap_mode_from_kw(kw: FlexWrapKw) -> FlexWrapMode {
    match kw {
        FlexWrapKw::NoWrap => FlexWrapMode::NoWrap,
        FlexWrapKw::Wrap => FlexWrapMode::Wrap,
        FlexWrapKw::WrapReverse => FlexWrapMode::WrapReverse,
    }
}

fn justify_content_from_kw(kw: JustifyContentKw) -> FlexMainAxisAlignment {
    match kw {
        JustifyContentKw::Start => FlexMainAxisAlignment::Start,
        JustifyContentKw::End => FlexMainAxisAlignment::End,
        JustifyContentKw::FlexStart => FlexMainAxisAlignment::FlexStart,
        JustifyContentKw::FlexEnd => FlexMainAxisAlignment::FlexEnd,
        JustifyContentKw::Center => FlexMainAxisAlignment::Center,
        JustifyContentKw::SpaceBetween => FlexMainAxisAlignment::SpaceBetween,
        JustifyContentKw::SpaceAround => FlexMainAxisAlignment::SpaceAround,
        JustifyContentKw::SpaceEvenly => FlexMainAxisAlignment::SpaceEvenly,
    }
}

fn align_items_from_kw(kw: AlignItemsKw) -> FlexCrossAxisAlignment {
    match kw {
        AlignItemsKw::Start => FlexCrossAxisAlignment::Start,
        AlignItemsKw::End => FlexCrossAxisAlignment::End,
        AlignItemsKw::FlexStart => FlexCrossAxisAlignment::FlexStart,
        AlignItemsKw::FlexEnd => FlexCrossAxisAlignment::FlexEnd,
        AlignItemsKw::Center => FlexCrossAxisAlignment::Center,
        AlignItemsKw::Stretch => FlexCrossAxisAlignment::Stretch,
    }
}

fn align_content_from_kw(kw: AlignContentKw) -> FlexLineCrossAxisAlignment {
    match kw {
        AlignContentKw::Start => FlexLineCrossAxisAlignment::Start,
        AlignContentKw::End => FlexLineCrossAxisAlignment::End,
        AlignContentKw::FlexStart => FlexLineCrossAxisAlignment::FlexStart,
        AlignContentKw::FlexEnd => FlexLineCrossAxisAlignment::FlexEnd,
        AlignContentKw::Center => FlexLineCrossAxisAlignment::Center,
        AlignContentKw::Stretch => FlexLineCrossAxisAlignment::Stretch,
        AlignContentKw::SpaceBetween => FlexLineCrossAxisAlignment::SpaceBetween,
        AlignContentKw::SpaceAround => FlexLineCrossAxisAlignment::SpaceAround,
        AlignContentKw::SpaceEvenly => FlexLineCrossAxisAlignment::SpaceEvenly,
    }
}

fn effective_flex_container_text_direction(node: &DomNode) -> DomTextDirection {
    node.computed_text().direction
}

pub(crate) fn item_align_self(node: &DomNode) -> Option<FlexCrossAxisAlignment> {
    node.computed_flex().align_self.map(align_items_from_kw)
}

fn flex_gap_sizes(gap: [f32; 2], main_axis: FlexAxis) -> FlexGapSizes {
    let [row_gap_px, column_gap_px] = gap;
    match main_axis {
        FlexAxis::Horizontal => FlexGapSizes {
            between_items_px: column_gap_px,
            between_lines_px: row_gap_px,
        },
        FlexAxis::Vertical => FlexGapSizes {
            between_items_px: row_gap_px,
            between_lines_px: column_gap_px,
        },
    }
}

fn inline_start_is_reversed(text_direction: DomTextDirection) -> bool {
    text_direction == DomTextDirection::Rtl
}

fn logical_main_axis_start_is_reversed(container_style: FlexContainerStyle) -> bool {
    match container_style.main_axis {
        FlexAxis::Horizontal => inline_start_is_reversed(container_style.text_direction),
        FlexAxis::Vertical => false,
    }
}

fn logical_cross_axis_start_is_reversed(container_style: FlexContainerStyle) -> bool {
    match container_style.main_axis {
        FlexAxis::Horizontal => false,
        FlexAxis::Vertical => inline_start_is_reversed(container_style.text_direction),
    }
}

pub(crate) fn resolved_main_axis_is_reversed(container_style: FlexContainerStyle) -> bool {
    match container_style.main_axis {
        FlexAxis::Horizontal => {
            container_style.is_main_reversed
                ^ inline_start_is_reversed(container_style.text_direction)
        }
        FlexAxis::Vertical => container_style.is_main_reversed,
    }
}

pub(crate) fn resolved_cross_axis_is_reversed(container_style: FlexContainerStyle) -> bool {
    logical_cross_axis_start_is_reversed(container_style)
        ^ container_style.wrap_mode.is_cross_reversed()
}

fn main_axis_logical_start_matches_flex_start(container_style: FlexContainerStyle) -> bool {
    logical_main_axis_start_is_reversed(container_style)
        == resolved_main_axis_is_reversed(container_style)
}

fn cross_axis_logical_start_matches_flex_start(container_style: FlexContainerStyle) -> bool {
    logical_cross_axis_start_is_reversed(container_style)
        == resolved_cross_axis_is_reversed(container_style)
}

pub(crate) fn resolved_main_axis_alignment(
    justify_content: FlexMainAxisAlignment,
    container_style: FlexContainerStyle,
) -> FlexMainAxisAlignment {
    match justify_content {
        FlexMainAxisAlignment::Start => {
            if main_axis_logical_start_matches_flex_start(container_style) {
                FlexMainAxisAlignment::FlexStart
            } else {
                FlexMainAxisAlignment::FlexEnd
            }
        }
        FlexMainAxisAlignment::End => {
            if main_axis_logical_start_matches_flex_start(container_style) {
                FlexMainAxisAlignment::FlexEnd
            } else {
                FlexMainAxisAlignment::FlexStart
            }
        }
        _ => justify_content,
    }
}

pub(crate) fn resolved_cross_axis_alignment(
    alignment: FlexCrossAxisAlignment,
    container_style: FlexContainerStyle,
) -> FlexCrossAxisAlignment {
    match alignment {
        FlexCrossAxisAlignment::Start => {
            if cross_axis_logical_start_matches_flex_start(container_style) {
                FlexCrossAxisAlignment::FlexStart
            } else {
                FlexCrossAxisAlignment::FlexEnd
            }
        }
        FlexCrossAxisAlignment::End => {
            if cross_axis_logical_start_matches_flex_start(container_style) {
                FlexCrossAxisAlignment::FlexEnd
            } else {
                FlexCrossAxisAlignment::FlexStart
            }
        }
        _ => alignment,
    }
}

pub(crate) fn resolved_line_cross_axis_alignment(
    align_content: FlexLineCrossAxisAlignment,
    container_style: FlexContainerStyle,
) -> FlexLineCrossAxisAlignment {
    match align_content {
        FlexLineCrossAxisAlignment::Start => {
            if cross_axis_logical_start_matches_flex_start(container_style) {
                FlexLineCrossAxisAlignment::FlexStart
            } else {
                FlexLineCrossAxisAlignment::FlexEnd
            }
        }
        FlexLineCrossAxisAlignment::End => {
            if cross_axis_logical_start_matches_flex_start(container_style) {
                FlexLineCrossAxisAlignment::FlexEnd
            } else {
                FlexLineCrossAxisAlignment::FlexStart
            }
        }
        _ => align_content,
    }
}

pub(crate) fn item_order_from_node(node: &DomNode) -> i32 {
    node.computed_flex().order
}

pub(crate) fn entered_container_main_size(
    entered: &EnteredNodeLayout,
    main_axis: FlexAxis,
) -> Option<f32> {
    match main_axis {
        FlexAxis::Horizontal => Some(entered.content_width_px),
        // The vertical main size reads the raw specified height, so it is
        // min/max-clamped here exactly as the block machinery does (flex arc
        // step 6): the same resolved `NodeStyle` min/max, the same clamp.
        FlexAxis::Vertical => entered.style.height_px.map(|height| {
            clamp_length(
                height,
                entered.style.min_height_px,
                entered.style.max_height_px,
            )
        }),
    }
}

pub(crate) fn entered_container_cross_size(
    entered: &EnteredNodeLayout,
    main_axis: FlexAxis,
) -> Option<f32> {
    match main_axis {
        FlexAxis::Horizontal => entered.style.height_px.map(|height| {
            clamp_length(
                height,
                entered.style.min_height_px,
                entered.style.max_height_px,
            )
        }),
        FlexAxis::Vertical => Some(entered.content_width_px),
    }
}

pub(crate) fn flex_line_gap_count(item_count: usize) -> usize {
    item_count.saturating_sub(1)
}

pub(crate) fn flex_axis_sizes_match(left_px: f32, right_px: f32) -> bool {
    (left_px - right_px).abs() <= f32::EPSILON
}

/// Converts a flex count (`usize`, non-negative by construction) to `f32`.
///
/// The former implementation round-tripped through a decimal `String`
/// (`to_string().parse::<f32>()`), allocating once per call on the hot solve
/// path. Both forms round the exact integer to the nearest `f32` (the
/// correctly-rounded value), so `usize_to_f32` — the crate's clippy-clean
/// `usize as f32` cast under `forbid(clippy::cast_precision_loss)` — is
/// bit-for-bit equivalent and allocation-free. Pinned by
/// `non_negative_f32_from_usize_matches_as_cast_bit_for_bit`.
pub(crate) fn non_negative_f32_from_usize(value: usize) -> f32 {
    usize_to_f32(value)
}

pub(crate) fn active_flex_content_width_px(
    main_axis: FlexAxis,
    main_size_px: f32,
    cross_size_px: f32,
) -> f32 {
    match main_axis {
        FlexAxis::Horizontal => main_size_px,
        FlexAxis::Vertical => cross_size_px,
    }
}

pub(crate) fn active_flex_content_height_px(
    main_axis: FlexAxis,
    main_size_px: f32,
    cross_size_px: f32,
) -> f32 {
    match main_axis {
        FlexAxis::Horizontal => cross_size_px,
        FlexAxis::Vertical => main_size_px,
    }
}

pub(crate) fn non_negative_flex_axis_size_px(size_px: f32) -> f32 {
    size_px.max(0.0)
}

pub(crate) fn cross_axis_flow_size(main_axis: FlexAxis, measured_flow: FlowSize) -> f32 {
    match main_axis {
        FlexAxis::Horizontal => measured_flow.height_px,
        FlexAxis::Vertical => measured_flow.width_px,
    }
}

pub(crate) fn main_axis_border_padding_size_px(sides: BoxSides, main_axis: FlexAxis) -> f32 {
    match main_axis {
        FlexAxis::Horizontal => {
            sides.border.left_px
                + sides.border.right_px
                + sides.padding.left_px
                + sides.padding.right_px
        }
        FlexAxis::Vertical => {
            sides.border.top_px
                + sides.border.bottom_px
                + sides.padding.top_px
                + sides.padding.bottom_px
        }
    }
}

pub(crate) fn cross_axis_border_padding_size_px(sides: BoxSides, main_axis: FlexAxis) -> f32 {
    match main_axis {
        FlexAxis::Horizontal => {
            sides.border.top_px
                + sides.border.bottom_px
                + sides.padding.top_px
                + sides.padding.bottom_px
        }
        FlexAxis::Vertical => {
            sides.border.left_px
                + sides.border.right_px
                + sides.padding.left_px
                + sides.padding.right_px
        }
    }
}

/// The item's four auto-margin flags (D1, flex arc step 10), read from the
/// computed box group's margin `BoxLength` fields: a side is auto iff its
/// computed margin is `BoxLength::Auto`. The computed box group's margin array
/// is `[top, right, bottom, left]` (group order); the return maps to
/// `(main_start, main_end, cross_start, cross_end)` per the item's main axis.
pub(crate) fn auto_margin_sides(
    margin: [BoxLength; 4],
    main_axis: FlexAxis,
) -> (bool, bool, bool, bool) {
    let top = matches!(margin[0], BoxLength::Auto);
    let right = matches!(margin[1], BoxLength::Auto);
    let bottom = matches!(margin[2], BoxLength::Auto);
    let left = matches!(margin[3], BoxLength::Auto);
    match main_axis {
        FlexAxis::Horizontal => (left, right, top, bottom),
        FlexAxis::Vertical => (top, bottom, left, right),
    }
}

pub(crate) fn main_axis_start_margin(style: NodeStyle, main_axis: FlexAxis) -> f32 {
    // `node_style` resolves an auto margin edge to a 0.0 placeholder; this
    // solver reads that placeholder as the PRE-absorption value. The flex
    // solver owns §8.1 (flex arc step 10): D2 distributes the line's positive
    // main-axis free space into the item view's auto main margins and D3 does
    // the same per item on the cross axis, both at positioning time — never at
    // measure/style time (the free space does not exist yet). The resolved
    // edges here stay 0 for auto margins and carry the authored value for
    // non-auto margins.
    main_axis_start_margin_from_sides(style.sides, main_axis)
}

pub(crate) fn main_axis_start_margin_from_sides(sides: BoxSides, main_axis: FlexAxis) -> f32 {
    match main_axis {
        FlexAxis::Horizontal => sides.margin.left_px,
        FlexAxis::Vertical => sides.margin.top_px,
    }
}

pub(crate) fn main_axis_end_margin(style: NodeStyle, main_axis: FlexAxis) -> f32 {
    main_axis_end_margin_from_sides(style.sides, main_axis)
}

pub(crate) fn main_axis_end_margin_from_sides(sides: BoxSides, main_axis: FlexAxis) -> f32 {
    match main_axis {
        FlexAxis::Horizontal => sides.margin.right_px,
        FlexAxis::Vertical => sides.margin.bottom_px,
    }
}

pub(crate) fn cross_axis_start_margin(style: NodeStyle, main_axis: FlexAxis) -> f32 {
    cross_axis_start_margin_from_sides(style.sides, main_axis)
}

pub(crate) fn cross_axis_start_margin_from_sides(sides: BoxSides, main_axis: FlexAxis) -> f32 {
    match main_axis {
        FlexAxis::Horizontal => sides.margin.top_px,
        FlexAxis::Vertical => sides.margin.left_px,
    }
}

pub(crate) fn cross_axis_end_margin(style: NodeStyle, main_axis: FlexAxis) -> f32 {
    cross_axis_end_margin_from_sides(style.sides, main_axis)
}

pub(crate) fn cross_axis_end_margin_from_sides(sides: BoxSides, main_axis: FlexAxis) -> f32 {
    match main_axis {
        FlexAxis::Horizontal => sides.margin.bottom_px,
        FlexAxis::Vertical => sides.margin.right_px,
    }
}

pub(crate) fn node_for_key(root_dom: &RootDom, key: DomNodeSlotKey) -> Result<&DomNode, DomError> {
    root_dom.nodes.get(key).ok_or(DomError::InvalidNodeKey)
}

// The flex item view's measure-path record accessors. The measurement record
// stores only basis-independent CONTENT sizes (min/max inline contributions
// and the content height at this pass's measure width); the item view re-adds
// the node's own border+padding at its production site, exactly as the
// pre-change `main_axis_border_box_size_from_flow`/`cross_axis_border_box_size_from_flow`
// recovered the border box from the folded `FlowSize`.

#[cfg(test)]
use super::MeasurementRecord;
#[cfg(test)]
use super::flex_formation::backfill_order_indices;
#[cfg(test)]
use super::flex_line_lane::FlexLineEntry;
#[cfg(test)]
use super::flex_measure::{
    build_staged_measurement_solve, record_content_cross_size, record_content_main_size,
    resolve_measured_flex_container,
};
#[cfg(test)]
use super::flex_order_lane::FlexOrderLaneEntry;
#[cfg(test)]
use super::flex_resolve::{
    build_measured_flex_lines, claim_resolution_range, line_cross_offsets_px, resolve_flex_lines,
    total_line_cross_size_px_from_lane, update_line_cross_sizes,
};
#[cfg(test)]
use super::flow::resolved_node_style;
#[cfg(test)]
use super::measurement_record::node_measurement_of;
#[cfg(test)]
use crate::domains::layout::IdentityDomLayoutResolver;
#[cfg(test)]
use crate::domains::layout::style::{layout_dirty_from_ref, node_box_from_ref};
#[cfg(test)]
use crate::domains::visitor::style_recompute::recompute_dirty_style_subtree_for_layer_root;
#[cfg(test)]
use crate::types::{
    DomBoxModel, DomIntrinsic, DomLayoutState, DomTag, LayoutScratch, RootDomContext,
};

#[cfg(test)]
pub(crate) fn test_keys(count: usize) -> Vec<DomNodeSlotKey> {
    let mut keys = slotmap::SlotMap::with_key();
    (0..count).map(|_| keys.insert(())).collect()
}

#[cfg(test)]
pub(crate) fn source(key: DomNodeSlotKey) -> FlexItemSource {
    FlexItemSource::BlockItem(key)
}

#[cfg(test)]
pub(crate) fn measured_row_item(
    source: FlexItemSource,
    flex_grow_factor: f32,
    flex_shrink_factor: f32,
    flex_base_size_px: f32,
    measured_cross_size_input_px: f32,
) -> FlexItemView {
    measured_row_item_with_automatic_min(
        source,
        flex_grow_factor,
        flex_shrink_factor,
        flex_base_size_px,
        measured_cross_size_input_px,
        0.0,
    )
}

#[cfg(test)]
pub(crate) fn measured_row_item_with_automatic_min(
    source: FlexItemSource,
    flex_grow_factor: f32,
    flex_shrink_factor: f32,
    flex_base_size_px: f32,
    measured_cross_size_input_px: f32,
    automatic_min_main_size_px: f32,
) -> FlexItemView {
    FlexItemView {
        source,
        order: 0,
        align_self: None,
        flex_grow_factor,
        flex_shrink_factor,
        flex_base_size_px,
        hypothetical_main_size_px: flex_base_size_px,
        automatic_min_main_size_px,
        min_main_size_px: None,
        max_main_size_px: None,
        main_axis_border_padding_px: 0.0,
        outer_hypothetical_main_size_px: flex_base_size_px,
        measured_cross_size_input_px,
        replaced_ratio: false,
        main_influences_cross: false,
        resolved_cross_size_px: measured_cross_size_input_px,
        h_min_content_px: automatic_min_main_size_px,
        h_max_content_px: flex_base_size_px,
        margin_main_start_px: 0.0,
        margin_main_end_px: 0.0,
        margin_cross_start_px: 0.0,
        margin_cross_end_px: 0.0,
        margin_main_start_auto: false,
        margin_main_end_auto: false,
        margin_cross_start_auto: false,
        margin_cross_end_auto: false,
    }
}

#[cfg(test)]
pub(crate) fn measured_row_item_with_limits(
    source: FlexItemSource,
    flex_grow_factor: f32,
    flex_shrink_factor: f32,
    flex_base_size_px: f32,
    measured_cross_size_input_px: f32,
    min_main_size_px: Option<f32>,
    max_main_size_px: Option<f32>,
) -> FlexItemView {
    let mut measured_item = measured_row_item(
        source,
        flex_grow_factor,
        flex_shrink_factor,
        flex_base_size_px,
        measured_cross_size_input_px,
    );
    measured_item.min_main_size_px = min_main_size_px;
    measured_item.max_main_size_px = max_main_size_px;
    measured_item.hypothetical_main_size_px = measured_item
        .hypothetical_main_size_px
        .max(min_main_size_px.unwrap_or(0.0));
    if let Some(max_main_size_px) = max_main_size_px {
        measured_item.hypothetical_main_size_px = measured_item
            .hypothetical_main_size_px
            .min(max_main_size_px);
    }
    measured_item.outer_hypothetical_main_size_px = measured_item.hypothetical_main_size_px;
    measured_item
}

#[cfg(test)]
#[derive(Clone, Copy)]
pub(crate) struct MeasuredRowItemBoxEdges {
    pub(crate) main_size: f32,
    pub(crate) cross_size: f32,
    pub(crate) main_border_padding: f32,
    pub(crate) main_start_margin: f32,
    pub(crate) main_end_margin: f32,
    pub(crate) cross_start_margin: f32,
    pub(crate) cross_end_margin: f32,
}

#[cfg(test)]
pub(crate) fn measured_row_item_with_box_edges(
    source: FlexItemSource,
    box_edges: MeasuredRowItemBoxEdges,
) -> FlexItemView {
    FlexItemView {
        source,
        order: 0,
        align_self: None,
        flex_grow_factor: 0.0,
        flex_shrink_factor: 0.0,
        flex_base_size_px: box_edges.main_size,
        hypothetical_main_size_px: box_edges.main_size,
        automatic_min_main_size_px: box_edges.main_size,
        min_main_size_px: None,
        max_main_size_px: None,
        main_axis_border_padding_px: box_edges.main_border_padding,
        outer_hypothetical_main_size_px: box_edges.main_size
            + box_edges.main_start_margin
            + box_edges.main_end_margin,
        measured_cross_size_input_px: box_edges.cross_size,
        replaced_ratio: false,
        main_influences_cross: false,
        resolved_cross_size_px: box_edges.cross_size,
        h_min_content_px: box_edges.main_size,
        h_max_content_px: box_edges.main_size,
        margin_main_start_px: box_edges.main_start_margin,
        margin_main_end_px: box_edges.main_end_margin,
        margin_cross_start_px: box_edges.cross_start_margin,
        margin_cross_end_px: box_edges.cross_end_margin,
        margin_main_start_auto: false,
        margin_main_end_auto: false,
        margin_cross_start_auto: false,
        margin_cross_end_auto: false,
    }
}

#[cfg(test)]
pub(crate) fn test_gap_sizes() -> FlexGapSizes {
    FlexGapSizes {
        between_items_px: 0.0,
        between_lines_px: 0.0,
    }
}

#[cfg(test)]
pub(crate) fn row_container_style() -> FlexContainerStyle {
    FlexContainerStyle {
        main_axis: FlexAxis::Horizontal,
        is_main_reversed: false,
        text_direction: DomTextDirection::Ltr,
        wrap_mode: FlexWrapMode::NoWrap,
        justify_content: FlexMainAxisAlignment::FlexStart,
        align_items: FlexCrossAxisAlignment::FlexStart,
        align_content: FlexLineCrossAxisAlignment::FlexStart,
        gap_sizes: test_gap_sizes(),
    }
}

#[cfg(test)]
pub(crate) fn wrap_row_container_style() -> FlexContainerStyle {
    FlexContainerStyle {
        wrap_mode: FlexWrapMode::Wrap,
        ..row_container_style()
    }
}

#[cfg(test)]
pub(crate) fn base_context() -> LayoutContext {
    LayoutContext {
        inherited_style: InheritedStyleContext::default(),
        flow_left_px: 0.0,
        flow_top_px: 0.0,
        root_local_left_px: None,
        root_local_top_px: None,
        available_width_px: Some(320.0),
        available_height_px: Some(320.0),
        auto_margin_reference_width_px: None,
        node_content_width_override_px: None,
        node_content_height_override_px: None,
        parent_absolute_left_px: 0.0,
        parent_absolute_top_px: 0.0,
        containing_layer_absolute_left_px: 0.0,
        containing_layer_absolute_top_px: 0.0,
        containing_layer_width_px: 320.0,
        containing_layer_height_px: 320.0,
        viewport_width_px: 320.0,
        viewport_height_px: 320.0,
        parent_box: None,
        allows_parent_child_block_margin_collapse: false,
        parent_is_flex_container: false,
        parent_is_grid_container: false,
    }
}

#[cfg(test)]
pub(crate) fn append_div(root: &mut RootDom, parent_key: DomNodeSlotKey) -> DomNodeSlotKey {
    let key = root.create_element(DomTag::Div).expect("div should insert");
    root.append_child(parent_key, key)
        .expect("div should append");
    key
}

#[cfg(test)]
pub(crate) fn append_span(root: &mut RootDom, parent_key: DomNodeSlotKey) -> DomNodeSlotKey {
    let key = root
        .create_element(DomTag::Span)
        .expect("span should insert");
    root.append_child(parent_key, key)
        .expect("span should append");
    key
}

#[cfg(test)]
pub(crate) fn append_text(
    root: &mut RootDom,
    parent_key: DomNodeSlotKey,
    text: &str,
) -> DomNodeSlotKey {
    let key = root.create_text_node(text).expect("text should insert");
    root.append_child(parent_key, key)
        .expect("text should append");
    key
}

#[cfg(test)]
pub(crate) fn flex_container_with_context() -> (RootDom, DomNodeSlotKey, LayoutContext) {
    let mut root = RootDom::new(RootDomContext::new_cpu());
    let document_key = root.document_node().0;
    let container = append_div(&mut root, document_key);
    root.style_mut(container)
        .set("display", "flex")
        .expect("display should set");
    (root, container, base_context())
}

#[cfg(test)]
pub(crate) fn collect_items_for_container(
    root: &mut RootDom,
    container: DomNodeSlotKey,
    context: LayoutContext,
) -> Vec<FlexItemView> {
    let document_key = root.document_node().0;
    recompute_dirty_style_subtree_for_layer_root(root, document_key).expect("style pass");
    let mut resolver = IdentityDomLayoutResolver;
    let (style, inherited_style) =
        resolved_node_style(root, container, &context).expect("container style");
    let solve = build_staged_measurement_solve(
        root,
        (320.0, 320.0),
        container,
        context,
        style,
        inherited_style,
        &mut resolver,
    )
    .expect("staged solve should build");
    item_views_in_dom_order(&root.layout_scratch, solve)
}

#[cfg(test)]
pub(crate) fn build_solve_for_container(
    root: &mut RootDom,
    container: DomNodeSlotKey,
    context: LayoutContext,
) -> FlexContainerSolve {
    let document_key = root.document_node().0;
    recompute_dirty_style_subtree_for_layer_root(root, document_key).expect("style pass");
    let mut resolver = IdentityDomLayoutResolver;
    let (style, inherited_style) =
        resolved_node_style(root, container, &context).expect("container style");
    build_staged_measurement_solve(
        root,
        (320.0, 320.0),
        container,
        context,
        style,
        inherited_style,
        &mut resolver,
    )
    .expect("staged solve should build")
}

#[cfg(test)]
pub(crate) fn node_layout(root: &RootDom, key: DomNodeSlotKey) -> DomLayoutState {
    match root.node(key).expect("node should exist") {
        DomNode::Document(node) => node.base.layout.clone(),
        DomNode::DocumentFragment(node) => node.base.layout.clone(),
        DomNode::Element(node) => node.base.layout.clone(),
        DomNode::Text(node) => node.base.layout.clone(),
    }
}

#[cfg(test)]
pub(crate) fn node_box(root: &RootDom, key: DomNodeSlotKey) -> DomBoxModel {
    node_box_from_ref(root.node(key).expect("node should exist"))
}

#[cfg(test)]
pub(crate) fn node_layout_dirty(root: &RootDom, key: DomNodeSlotKey) -> bool {
    layout_dirty_from_ref(root.node(key).expect("node should exist"))
}

#[cfg(test)]
/// A line's summary as the tests assert it (the former `FlexLine` shape,
/// reconstructed from the line lane's order-lane range).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TestLine {
    pub(crate) item_sources: Vec<FlexItemSource>,
    pub(crate) outer_hypothetical_main_size_px: f32,
    pub(crate) cross_size_px: f32,
}

#[cfg(test)]
/// The scratch-only resolve reorder the synthetic-item tests drive (the
/// production `resolve_active_flex_container` adds the refold before the
/// line-cross update). Synthetic items never refold (no flag, no ratio), so
/// this wrapper runs main resolve → item cross (measured) → line cross →
/// align-content without a `RootDom`.
pub(crate) fn resolve_active_flex_container_test(
    scratch: &mut LayoutScratch,
    measured_solve: FlexContainerSolve,
    container_style: FlexContainerStyle,
    container_main_size_px: f32,
    container_cross_size_px: Option<f32>,
) -> FlexContainerSolve {
    let order_range = measured_solve.order_range;
    let line_range = measured_solve.line_range;
    let resolution_range = claim_resolution_range(scratch, order_range);
    let mut solve = measured_solve;
    solve.resolution_range = resolution_range;
    resolve_flex_lines(
        scratch,
        order_range,
        resolution_range,
        line_range,
        container_main_size_px,
        container_style,
    );
    update_line_cross_sizes(scratch, order_range, line_range);
    super::flex_resolve::resolved_line_cross_sizes(
        scratch,
        line_range,
        container_style,
        container_cross_size_px,
    );
    line_cross_offsets_px(
        scratch,
        line_range,
        container_style,
        container_cross_size_px,
    );
    solve.main_size_px = container_main_size_px;
    solve.cross_size_px = total_line_cross_size_px_from_lane(
        scratch,
        line_range,
        container_style.gap_sizes.between_lines_px,
    );
    solve
}

#[cfg(test)]
pub(crate) fn item_views_in_dom_order(
    scratch: &LayoutScratch,
    solve: FlexContainerSolve,
) -> Vec<FlexItemView> {
    scratch
        .items
        .slice(solve.item_range)
        .iter()
        .map(|entry| entry.view)
        .collect()
}

#[cfg(test)]
pub(crate) fn item_views_in_order_modified_order(
    scratch: &LayoutScratch,
    solve: FlexContainerSolve,
) -> Vec<FlexItemView> {
    scratch
        .order
        .slice(solve.order_range)
        .iter()
        .map(|entry| scratch.items.get(entry.dom_index).view)
        .collect()
}

#[cfg(test)]
pub(crate) fn resolved_items_in_order_modified_order(
    scratch: &LayoutScratch,
    solve: FlexContainerSolve,
) -> Vec<ResolvedFlexItem> {
    scratch
        .order
        .slice(solve.order_range)
        .iter()
        .map(|order_entry| {
            let item_entry = scratch.items.get(order_entry.dom_index);
            let view = item_entry.view;
            let resolution = scratch.resolution.get(item_entry.order_index);
            let line = scratch.lines.get(item_entry.line_index);
            ResolvedFlexItem {
                source: view.source,
                align_self: view.align_self,
                resolved_main_size_px: resolution.resolved_main_size_px,
                main_axis_border_padding_px: view.main_axis_border_padding_px,
                resolved_cross_size_px: view.resolved_cross_size_px,
                replaced_ratio: view.replaced_ratio,
                main_offset_px: item_entry.main_offset_px,
                cross_offset_px: line.cross_offset_px,
            }
        })
        .collect()
}

#[cfg(test)]
pub(crate) fn resolved_line_cross_sizes_in_solve(
    scratch: &LayoutScratch,
    solve: FlexContainerSolve,
) -> Vec<f32> {
    scratch
        .lines
        .slice(solve.line_range)
        .iter()
        .map(|line| line.resolved_cross_size_px)
        .collect()
}

#[cfg(test)]
pub(crate) fn built_lines(
    scratch: &LayoutScratch,
    order_range: LaneRange,
    line_range: LaneRange,
) -> Vec<TestLine> {
    let order_entries = scratch.order.slice(order_range);
    scratch
        .lines
        .slice(line_range)
        .iter()
        .map(|line| {
            let start = usize::try_from(line.first_order_index - order_range.start)
                .expect("line first order index should be within the order range");
            let item_sources = order_entries[start
                ..start
                    + usize::try_from(line.item_count)
                        .expect("line item count should fit into usize")]
                .iter()
                .map(|entry| FlexItemSource::BlockItem(entry.child_key))
                .collect();
            TestLine {
                item_sources,
                outer_hypothetical_main_size_px: line.outer_hypothetical_main_size_px,
                cross_size_px: line.cross_size_px,
            }
        })
        .collect()
}

#[cfg(test)]
/// Pushes synthetic items into the item and order lanes and returns the
/// resulting ranges (used by the synthetic `resolve_active_flex_container`
/// and line-building tests, which have no real DOM).
pub(crate) fn push_items(
    scratch: &mut LayoutScratch,
    items: &[FlexItemView],
) -> (LaneRange, LaneRange) {
    let item_start = scratch.items.begin_range();
    let order_start = scratch.order.begin_range();
    for item in items {
        let dom_index = scratch.items.push(*item);
        scratch.order.push_in_order(FlexOrderLaneEntry {
            dom_index,
            child_key: item.source.key(),
            order: item.order,
        });
    }
    let item_range = scratch.items.finish_range(item_start);
    let order_range = scratch.order.finish_range(order_start);
    backfill_order_indices(scratch, order_range);
    (item_range, order_range)
}

#[cfg(test)]
/// Builds a lane-based `FlexContainerSolve` from synthetic items and the
/// lines they belong to, mirroring the former
/// `resolve_measured_flex_container(items, items, lines)`.
pub(crate) fn solve_from_items_and_lines(
    scratch: &mut LayoutScratch,
    items: &[FlexItemView],
    lines: &[TestLine],
) -> FlexContainerSolve {
    let (item_range, order_range) = push_items(scratch, items);
    let line_start = scratch.lines.begin_range();
    for line in lines {
        let order_entries = scratch.order.slice(order_range);
        let first = order_entries
            .iter()
            .position(|entry| entry.child_key == line.item_sources[0].key())
            .expect("line source should be present in the order lane");
        scratch.lines.push(FlexLineEntry {
            first_order_index: order_range.start
                + u32::try_from(first).expect("line first order index should fit into u32"),
            item_count: u32::try_from(line.item_sources.len())
                .expect("line item count should fit into u32"),
            outer_hypothetical_main_size_px: line.outer_hypothetical_main_size_px,
            cross_size_px: line.cross_size_px,
            cross_offset_px: 0.0,
            resolved_cross_size_px: 0.0,
            placed_cross_size_px: 0.0,
        });
    }
    let line_range = scratch.lines.finish_range(line_start);
    resolve_measured_flex_container(scratch, item_range, order_range, line_range)
}

#[cfg(test)]
/// Builds lines over synthetic items (the former
/// `build_measured_flex_lines(&items, ...)` test seam).
pub(crate) fn build_lines_from_items(
    scratch: &mut LayoutScratch,
    items: &[FlexItemView],
    wrap_mode: FlexWrapMode,
    container_main_size_px: Option<f32>,
    gap_between_items_px: f32,
) -> Vec<TestLine> {
    let (_, order_range) = push_items(scratch, items);
    let line_range = build_measured_flex_lines(
        scratch,
        order_range,
        wrap_mode,
        container_main_size_px,
        gap_between_items_px,
    );
    built_lines(scratch, order_range, line_range)
}

#[cfg(test)]
pub(crate) fn configure_row_reverse_item(root: &mut RootDom, item: DomNodeSlotKey) {
    root.style_mut(item)
        .set("flex-direction", "row-reverse")
        .expect("row-reverse should set");
}

#[cfg(test)]
pub(crate) fn configure_wrap_item(root: &mut RootDom, item: DomNodeSlotKey) {
    root.style_mut(item)
        .set("flex-wrap", "wrap")
        .expect("wrap should set");
}

#[cfg(test)]
pub(crate) fn configure_rtl_row_item(root: &mut RootDom, item: DomNodeSlotKey) {
    root.style_mut(item)
        .set("direction", "rtl")
        .expect("rtl should set");
}

#[cfg(test)]
pub(crate) fn node_record(root: &RootDom, key: DomNodeSlotKey) -> MeasurementRecord {
    node_measurement_of(root.node(key).expect("node should exist"))
}

#[cfg(test)]
pub(crate) fn text_intrinsic_widths(root: &RootDom, key: DomNodeSlotKey) -> (f32, f32) {
    let DomIntrinsic::Text(text_intrinsic) = root.node(key).expect("text node").intrinsic() else {
        panic!("text node should carry a measured text intrinsic");
    };
    (
        text_intrinsic.min_content_width_px,
        text_intrinsic.max_content_width_px,
    )
}

#[cfg(test)]
/// The text node's retained fragment surface: (min-content, max-content,
/// strut line-height max) — the three values the run fold consumes.
pub(crate) fn text_intrinsic_triple(root: &RootDom, key: DomNodeSlotKey) -> (f32, f32, f32) {
    let DomIntrinsic::Text(text_intrinsic) = root.node(key).expect("text node").intrinsic() else {
        panic!("text node should carry a measured text intrinsic");
    };
    (
        text_intrinsic.min_content_width_px,
        text_intrinsic.max_content_width_px,
        text_intrinsic.fragment_line_height_max_px,
    )
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
/// Asserts an auto-basis item view's CONTENT-SIZE fields are the record's
/// content sizes plus the node's own border/padding, exactly as the
/// pre-change `main_axis_border_box_size_from_flow`/`cross_axis_border_box_size_from_flow`
/// recovered the border box from the folded `FlowSize`.
pub(crate) fn assert_item_view_reads_record(
    item: &FlexItemView,
    record: MeasurementRecord,
    main_axis: FlexAxis,
    main_border_padding_px: f32,
    cross_border_padding_px: f32,
) {
    assert_eq!(
        item.flex_base_size_px,
        record_content_main_size(main_axis, record) + main_border_padding_px,
        "auto-basis flex base size should be the record's main content size plus border/padding",
    );
    assert_eq!(
        item.measured_cross_size_input_px,
        record_content_cross_size(main_axis, record) + cross_border_padding_px,
        "measured cross size should be the record's cross content size plus border/padding",
    );
    assert_eq!(
        item.hypothetical_main_size_px, item.flex_base_size_px,
        "unclamped auto-basis hypothetical main size should equal the flex base size",
    );
    assert_eq!(
        item.outer_hypothetical_main_size_px,
        item.hypothetical_main_size_px + item.margin_main_start_px + item.margin_main_end_px,
    );
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn single_line_flex_content_sizes_map_vertical_axis_into_width_and_height() {
        assert_eq!(
            active_flex_content_width_px(FlexAxis::Horizontal, 90.0, 18.0),
            90.0,
        );
        assert_eq!(
            active_flex_content_height_px(FlexAxis::Horizontal, 90.0, 18.0),
            18.0,
        );
        assert_eq!(
            active_flex_content_width_px(FlexAxis::Vertical, 90.0, 18.0),
            18.0,
        );
        assert_eq!(
            active_flex_content_height_px(FlexAxis::Vertical, 90.0, 18.0),
            90.0,
        );
    }

    #[test]
    fn non_negative_f32_from_usize_matches_as_cast_bit_for_bit() {
        // The former `to_string().parse::<f32>()` implementation rounded the
        // exact decimal representation to nearest; the new `usize as f32`
        // conversion rounds the exact integer to nearest. Both are
        // correctly-rounded conversions of the same exact value, so they must
        // agree bit-for-bit across the domain — including values around 2^24
        // (the first non-representable integer in f32) and `usize::MAX`.
        fn old_behavior(value: usize) -> f32 {
            value
                .to_string()
                .parse::<f32>()
                .expect("usize-backed flex count should parse as f32")
        }
        for value in [
            0usize,
            1,
            2,
            3,
            16,
            1_000,
            8_388_607,  // 2^23 - 1
            8_388_608,  // 2^23
            16_777_215, // 2^24 - 1
            16_777_216, // 2^24
            16_777_217, // 2^24 + 1
            1_000_000_000,
            usize::MAX / 4 + 1,
            usize::MAX / 2,
            usize::MAX - 1,
            usize::MAX,
        ] {
            assert_eq!(
                non_negative_f32_from_usize(value).to_bits(),
                old_behavior(value).to_bits(),
                "non_negative_f32_from_usize({value}) should round to the same f32 bits as the decimal-string parse",
            );
        }
    }
}
