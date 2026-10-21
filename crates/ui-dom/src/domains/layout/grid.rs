//! Grid solver entrypoint for the shared `TreeWalker`.
//!
//! This file owns the T1 card-wall solver (`GRID_STEP2_DESIGN.md`): it replaces
//! the T0 block interim with the real solver. It follows the flex solver's
//! enter/measure/resolve/place/exit shape: children form grid items via the 3a
//! mechanics (run aggregation, exclusion, blockification), the explicit column
//! tracks size per the engines' phase order (init → intrinsic resolution →
//! maximize → fr expand → stretch auto), items fill row-major into the explicit
//! columns (order-modified, wrapping to implicit rows), implicit rows size to
//! content (the record, with the refold for width-dependent items), items
//! stretch to their cell by default, abs-pos children get the padding-box
//! static-position model, and the container's intrinsic sizes derive from the
//! solved tracks (D7).
//!
//! Invariants and rules:
//!
//! - grid solving stays inside the shared walker and shared enter/exit pattern
//! - transient solve state lives only in the scratch lanes (grid item lane,
//!   order lane, track lane) — never persisted on nodes, never allocated past
//!   the high-water mark
//! - `GridItemView` is plain data (no `impl`); its behavior lives in this
//!   file's free functions
//! - the D2 track sizing phases operate in place on the track lane's column
//!   range, exactly: init → maximize (minmax tracks with finite limits) → fr
//!   expand (CSS Grid 1 §7.2.4's freeze-and-redistribute loop, with the sum<1
//!   cap) → stretch auto (auto tracks absorb leftover equally)
#![allow(clippy::needless_pass_by_value)] // LayoutContext is passed by value as a child's owned context, mirroring flow.rs/flex.rs

use crate::domains::style::computed::{BoxLength, GridTrackSize, TrackBreadth, TrackMaxBreadth};
use crate::domains::style::{GRID_TRACK_LIST_CAPACITY, GridTrackListKey};
use crate::domains::visitor::inherited_style::InheritedStyleContext;
use crate::numeric::usize_to_f32;
use crate::types::{
    DomBoxModel, DomError, DomNode, DomNodeSlotKey, DomRect, LaneRange, LayoutScratch, RootDom,
    RootDomMetricName,
};

use super::DomLayoutResolver;
use super::flex::{
    measure_inline_run, measure_tree_walker_node_without_layout_write,
    node_has_main_influences_cross_child, write_node_border_box_position,
};
use super::flex_anonymous_run::{
    RunAccumulator, container_mode_is_collapse, node_is_excluded_flex_child,
    node_is_inline_run_member,
};
use super::flex_item_source::FlexItemSource;
use super::flex_order_lane::FlexOrderLaneEntry;
use super::flow::{
    EnteredNodeLayout, FlowSize, LayoutContext, auto_content_width_for_exit, border_box_height,
    border_box_width, child_layout_context, display_is_inline_outer, enter_node,
    entered_content_size_property_style, exit_node, first_child_key, layout_flow_size,
    next_sibling_key, relayout_tree_walker_node, resolved_content_size_px, resolved_node_style,
};
use super::grid_item_lane::GridItemLaneEntry;
use super::grid_track_lane::GridTrackEntry;
use super::measurement_record::{
    compute_node_measurement, node_measurement_of, write_measurement_if_changed,
};
use super::style::{BoxSides, LayoutDisplay, NodeStyle, PositionMode, presented_position};

/// The auto-margin bit positions in `GridItemView::auto_margins` (bit 0 = top,
/// bit 1 = right, bit 2 = bottom, bit 3 = left — the box group's
/// `[top, right, bottom, left]` order).
const MARGIN_TOP_AUTO: u8 = 0b0000_0001;
const MARGIN_RIGHT_AUTO: u8 = 0b0000_0010;
const MARGIN_BOTTOM_AUTO: u8 = 0b0000_0100;
const MARGIN_LEFT_AUTO: u8 = 0b0000_1000;

/// The track state bit marking an `auto` max breadth (stretch-auto eligible).
const GRID_TRACK_STATE_AUTO: u8 = 0b0000_0001;

/// The grid item view the solve consumes in place of the flex view. Its CONTENT
/// sizes (`content_main_px`/`content_cross_px` and `h_min`/`h_max`) are derived
/// from the child's measurement record (written by the measure fold just before
/// this view is assembled); margins + auto flags are read from the node's
/// computed box group. `content_main_px` is the item's natural content width
/// (the record's max-content inline), `content_cross_px` its content height at
/// the measure width.
#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(clippy::struct_excessive_bools)] // the ratio/flag booleans are named semantic bits (D1)
pub(crate) struct GridItemView {
    pub(crate) source: FlexItemSource,
    pub(crate) order: i32,
    /// The item's natural content width (the record's max-content inline).
    pub(crate) content_main_px: f32,
    /// The item's content height at the measure width.
    pub(crate) content_cross_px: f32,
    /// Min-content inline size (`MinContent` track contribution).
    pub(crate) h_min_content_px: f32,
    /// Max-content inline size (MaxContent/Auto/Fr track contribution).
    pub(crate) h_max_content_px: f32,
    /// Resolved margins `[top, right, bottom, left]` (auto sides carry 0.0).
    pub(crate) margins: [f32; 4],
    /// Per-side auto-margin flags (`MARGIN_*_AUTO` bits).
    pub(crate) auto_margins: u8,
    /// Whether the item is a replaced element with an intrinsic aspect ratio
    /// (its cross follows the ratio from its resolved width, not stretched).
    pub(crate) replaced_ratio: bool,
    /// Whether the item's cross may change when its width changes (text wrap /
    /// percentage cross) — a flagged item whose resolved width differs refolds.
    pub(crate) main_influences_cross: bool,
}

/// The per-container grid solve: the three lane ranges plus the resolved
/// container content height.
#[derive(Clone, Copy, Debug, PartialEq)]
struct GridContainerSolve {
    item_range: LaneRange,
    order_range: LaneRange,
    column_range: LaneRange,
    row_range: LaneRange,
    content_height_px: f32,
}

/// The placement result for one grid child: the resolved child context plus the
/// four absorbed auto margins (to write back into the item's box model).
struct GridChildPlacement {
    context: LayoutContext,
    absorbed_left_px: f32,
    absorbed_right_px: f32,
    absorbed_top_px: f32,
    absorbed_bottom_px: f32,
}

pub(super) fn relayout_grid_node<R>(
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
    relayout_active_grid_node(
        root_dom,
        tree_size,
        key,
        context,
        style,
        inherited_style,
        resolver,
    )
}

#[allow(clippy::too_many_lines)] // the solver is one linear phase sequence, mirroring flex's relayout_active_flex_node
fn relayout_active_grid_node<R>(
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
    // Enter + container probes.
    let (grid_group, column_gap_px, row_gap_px) = {
        let container_node = root_dom.nodes.get(key).ok_or(DomError::InvalidNodeKey)?;
        let gap = container_node.computed_flex().gap;
        (container_node.stored_computed_groups().grid, gap[1], gap[0])
    };
    let mut column_tracks = empty_track_array();
    let col_count = resolve_columns(root_dom, grid_group.template_columns, &mut column_tracks);

    let entered = enter_node(
        root_dom,
        tree_size,
        key,
        context.clone(),
        style,
        inherited_style,
        resolver,
    )?;
    let child_context = child_layout_context(context.clone(), entered.clone());

    // 1. Item formation (measure fold).
    let (item_range, order_range) = build_grid_item_formation(
        root_dom,
        tree_size,
        key,
        &child_context,
        resolver,
        RootDomMetricName::GridNodesMeasured,
    )?;

    // 2. Per-column contributions (max h_min / h_max per auto-placed column).
    let (col_h_min, col_h_max) = column_contributions(root_dom, order_range, col_count);

    // 3. Container content width (block → CB width; InlineGrid → D7 intrinsic).
    let container_content_width = grid_container_content_width(
        &entered,
        &column_tracks,
        col_count,
        &col_h_min,
        &col_h_max,
        column_gap_px,
    );

    // 4. Column track sizing.
    let column_range = size_column_tracks(
        &mut root_dom.layout_scratch,
        &column_tracks,
        col_count,
        &col_h_min,
        &col_h_max,
        container_content_width,
        column_gap_px,
    );

    // 5. Item resolution (resolved width + cross contribution, refold).
    resolve_grid_items(
        root_dom,
        tree_size,
        &child_context,
        order_range,
        col_count,
        column_range,
        resolver,
        RootDomMetricName::GridNodesMeasured,
    )?;

    // 6. Row sizing (implicit rows).
    let row_count = implicit_row_count(order_range, col_count);
    let row_range = size_implicit_rows(
        &mut root_dom.layout_scratch,
        order_range,
        col_count,
        row_count,
        grid_group.auto_rows,
        entered.style.height_px,
    );
    let content_height_px = total_row_height(&root_dom.layout_scratch, row_range, row_gap_px);

    // 7. Placement + abs-pos pass.
    let placed_content_height_px = place_grid_children(
        root_dom,
        tree_size,
        &child_context,
        GridContainerSolve {
            item_range,
            order_range,
            column_range,
            row_range,
            content_height_px,
        },
        column_gap_px,
        row_gap_px,
        resolver,
    )?;
    place_excluded_abs_grid_children(
        root_dom,
        tree_size,
        key,
        &entered,
        &child_context,
        container_content_width,
        placed_content_height_px,
        resolver,
    )?;

    // 8. Container intrinsic contribution (D7).
    let (h_min, h_max) = grid_container_intrinsic_inline_sizes(
        &column_tracks,
        col_count,
        &col_h_min,
        &col_h_max,
        column_gap_px,
        Some(container_content_width),
    );

    let auto_content_width_px = auto_content_width_for_exit(
        &entered,
        context.available_width_px,
        container_content_width,
        h_min,
        h_max,
    );
    exit_node(
        root_dom,
        key,
        context,
        entered,
        auto_content_width_px,
        (h_min, h_max),
        placed_content_height_px,
    )
}

/// A fresh `GridTrackSize` array filled with the `auto` track sentinel.
fn empty_track_array() -> [GridTrackSize; GRID_TRACK_LIST_CAPACITY] {
    [GridTrackSize {
        min: TrackBreadth::Auto,
        max: TrackMaxBreadth::Auto,
    }; GRID_TRACK_LIST_CAPACITY]
}

/// Copies the interned explicit column list into the caller's stack array and
/// returns the real column count (1 for the empty template — D3).
fn resolve_columns(
    root_dom: &RootDom,
    key: GridTrackListKey,
    column_tracks: &mut [GridTrackSize; GRID_TRACK_LIST_CAPACITY],
) -> usize {
    let mut count = 0;
    if let Some(list) = root_dom.resolve_inline_track_list(key) {
        for (slot, track) in column_tracks.iter_mut().zip(list.iter()) {
            *slot = *track;
            count += 1;
        }
    }
    count.max(1)
}

/// The grid container's content width: the containing-block width for a
/// block-level `Grid`, or the D7 intrinsic max-content for `InlineGrid`
/// (correction 4 — percent tracks resolve content-based when the container's
/// own width is indefinite at shrink-to-fit time).
fn grid_container_content_width(
    entered: &EnteredNodeLayout,
    column_tracks: &[GridTrackSize; GRID_TRACK_LIST_CAPACITY],
    col_count: usize,
    col_h_min: &[f32; GRID_TRACK_LIST_CAPACITY],
    col_h_max: &[f32; GRID_TRACK_LIST_CAPACITY],
    column_gap_px: f32,
) -> f32 {
    if !display_is_inline_outer(entered.style.display) {
        return entered.content_width_px;
    }
    grid_intrinsic_inline_size_impl(
        column_tracks,
        col_count,
        col_h_min,
        col_h_max,
        column_gap_px,
        true,
        None,
    )
}

/// Builds the measure fold: claims disjoint grid-item and order lane ranges,
/// pushes each child in DOM order (measure → item lane → order lane), forming
/// anonymous inline runs and excluding abs-pos/`display: none` children per the
/// 3a mechanics. Returns the two ranges.
fn build_grid_item_formation<R>(
    root_dom: &mut RootDom,
    tree_size: (f32, f32),
    key: DomNodeSlotKey,
    child_context: &LayoutContext,
    resolver: &mut R,
    metric: RootDomMetricName,
) -> Result<(LaneRange, LaneRange), DomError>
where
    R: DomLayoutResolver + ?Sized,
{
    let item_range_start = root_dom.layout_scratch.grid_items.begin_range();
    let order_range_start = root_dom.layout_scratch.order.begin_range();
    let white_space = child_context.inherited_style.effective_white_space();
    let mut child_key = first_child_key(root_dom, key)?;
    while let Some(current_child_key) = child_key {
        let current_node = root_dom
            .nodes
            .get(current_child_key)
            .ok_or(DomError::InvalidNodeKey)?;
        if node_is_inline_run_member(current_node) {
            let (run, run_breaker_key, measured_joint_height_px) = measure_inline_run(
                root_dom,
                tree_size,
                current_child_key,
                child_context,
                white_space,
                resolver,
                metric,
            )?;
            if !(run.all_members_text_and_all_whitespace && container_mode_is_collapse(white_space))
            {
                let source = FlexItemSource::AnonymousInlineGroup {
                    first_child_key: run.first_child_key,
                    child_count: run.child_count,
                };
                let item =
                    anonymous_inline_run_grid_item_view(source, run, measured_joint_height_px);
                push_grid_item_into_lanes(
                    &mut root_dom.layout_scratch,
                    item,
                    0.0,
                    0.0,
                    current_child_key,
                );
            }
            child_key = run_breaker_key;
        } else if node_is_excluded_flex_child(current_node) {
            child_key = next_sibling_key(root_dom, current_child_key)?;
        } else {
            let next_child_key = next_sibling_key(root_dom, current_child_key)?;
            let source = FlexItemSource::BlockItem(current_child_key);
            let (item, horizontal_bp, vertical_bp) = measure_one_grid_item(
                root_dom,
                tree_size,
                source,
                child_context,
                resolver,
                metric,
            )?;
            push_grid_item_into_lanes(
                &mut root_dom.layout_scratch,
                item,
                horizontal_bp,
                vertical_bp,
                current_child_key,
            );
            child_key = next_child_key;
        }
    }
    let item_range = root_dom
        .layout_scratch
        .grid_items
        .finish_range(item_range_start);
    let order_range = root_dom
        .layout_scratch
        .order
        .finish_range(order_range_start);
    Ok((item_range, order_range))
}

/// Pushes one assembled item view (with its border/padding insets) into the
/// grid item lane and its order-lane entry.
fn push_grid_item_into_lanes(
    scratch: &mut LayoutScratch,
    item: GridItemView,
    horizontal_bp: f32,
    vertical_bp: f32,
    child_key: DomNodeSlotKey,
) {
    let dom_index = scratch.grid_items.push(item, horizontal_bp, vertical_bp);
    scratch.order.push_in_order(FlexOrderLaneEntry {
        dom_index,
        child_key,
        order: item.order,
    });
}

/// The virtual grid item view for an anonymous inline run (the flex run item's
/// grid analog): order 0, zero margins/border/padding, no auto margins, not
/// replaced, not flagged. Content sizes come from the run accumulator, NOT a
/// node record (there is no node).
fn anonymous_inline_run_grid_item_view(
    source: FlexItemSource,
    run: RunAccumulator,
    measured_joint_height_px: f32,
) -> GridItemView {
    GridItemView {
        source,
        order: 0,
        content_main_px: run.max_content_sum_px,
        content_cross_px: measured_joint_height_px,
        h_min_content_px: run.widest_segment_px,
        h_max_content_px: run.max_content_sum_px,
        margins: [0.0; 4],
        auto_margins: 0,
        replaced_ratio: false,
        main_influences_cross: false,
    }
}

/// CSS Display 3 §2.7: blockify a grid item's `inline-grid` outer display to
/// `grid`, so a nested inline-grid item resolves its auto size like a
/// block-level grid (fills the cell) rather than shrink-wrapping.
fn blockified_grid_item_style(mut style: NodeStyle) -> NodeStyle {
    if style.display == LayoutDisplay::InlineGrid {
        style.display = LayoutDisplay::Grid;
    }
    style
}

/// Measures one grid item (blockified) and assembles its view. Returns the view
/// plus the item's horizontal/vertical border+padding insets.
fn measure_one_grid_item<R>(
    root_dom: &mut RootDom,
    tree_size: (f32, f32),
    source: FlexItemSource,
    child_context: &LayoutContext,
    resolver: &mut R,
    metric: RootDomMetricName,
) -> Result<(GridItemView, f32, f32), DomError>
where
    R: DomLayoutResolver + ?Sized,
{
    let child_key = source.key();
    let (child_style, child_inherited_style) =
        resolved_node_style(root_dom, child_key, child_context)?;
    let child_style = blockified_grid_item_style(child_style);
    measure_tree_walker_node_without_layout_write(
        root_dom,
        tree_size,
        child_key,
        child_context,
        None,
        child_style,
        child_inherited_style,
        resolver,
        metric,
    )?;
    let node = root_dom
        .nodes
        .get(child_key)
        .ok_or(DomError::InvalidNodeKey)?;
    let record = node_measurement_of(node);
    let order = node.computed_flex().order;
    let auto_margins = auto_margin_bits(node.computed_box().margin);
    let horizontal_bp = horizontal_border_padding_px(child_style.sides);
    let vertical_bp = vertical_border_padding_px(child_style.sides);
    let view = GridItemView {
        source,
        order,
        content_main_px: record.h_max_content_px,
        content_cross_px: record.content_height_at_width_px,
        h_min_content_px: record.h_min_content_px,
        h_max_content_px: record.h_max_content_px,
        margins: [
            child_style.sides.margin.top_px,
            child_style.sides.margin.right_px,
            child_style.sides.margin.bottom_px,
            child_style.sides.margin.left_px,
        ],
        auto_margins,
        replaced_ratio: record.flags.replaced_ratio(),
        main_influences_cross: record.flags.main_influences_cross(),
    };
    Ok((view, horizontal_bp, vertical_bp))
}

/// The item's four auto-margin bits, read from the computed box group's margin
/// `BoxLength` fields (`[top, right, bottom, left]`).
fn auto_margin_bits(margin: [BoxLength; 4]) -> u8 {
    let mut bits = 0u8;
    if matches!(margin[0], BoxLength::Auto) {
        bits |= MARGIN_TOP_AUTO;
    }
    if matches!(margin[1], BoxLength::Auto) {
        bits |= MARGIN_RIGHT_AUTO;
    }
    if matches!(margin[2], BoxLength::Auto) {
        bits |= MARGIN_BOTTOM_AUTO;
    }
    if matches!(margin[3], BoxLength::Auto) {
        bits |= MARGIN_LEFT_AUTO;
    }
    bits
}

fn horizontal_border_padding_px(sides: BoxSides) -> f32 {
    sides.border.left_px + sides.border.right_px + sides.padding.left_px + sides.padding.right_px
}

fn vertical_border_padding_px(sides: BoxSides) -> f32 {
    sides.border.top_px + sides.border.bottom_px + sides.padding.top_px + sides.padding.bottom_px
}

/// Computes the per-column max `h_min`/`h_max` item contributions, grouping
/// order-lane positions by `col = index % col_count` (D2's span model reduced
/// to the all-auto row-major fill).
fn column_contributions(
    root_dom: &RootDom,
    order_range: LaneRange,
    col_count: usize,
) -> (
    [f32; GRID_TRACK_LIST_CAPACITY],
    [f32; GRID_TRACK_LIST_CAPACITY],
) {
    let mut col_h_min = [0.0f32; GRID_TRACK_LIST_CAPACITY];
    let mut col_h_max = [0.0f32; GRID_TRACK_LIST_CAPACITY];
    let order_entries = root_dom.layout_scratch.order.slice(order_range);
    for (index, entry) in order_entries.iter().enumerate() {
        let col = index % col_count.max(1);
        let view = root_dom.layout_scratch.grid_items.get(entry.dom_index).view;
        col_h_min[col] = col_h_min[col].max(view.h_min_content_px);
        col_h_max[col] = col_h_max[col].max(view.h_max_content_px);
    }
    (col_h_min, col_h_max)
}

/// Initializes one column track's base/limit/flex from its `GridTrackSize` and
/// the track's item contributions.
fn init_column_track(
    track: &GridTrackSize,
    h_min: f32,
    h_max: f32,
    container_width_px: f32,
) -> GridTrackEntry {
    let is_auto = matches!(track.max, TrackMaxBreadth::Auto);
    let base_px = resolve_track_min_breadth(track.min, h_min, h_max, container_width_px);
    let limit_px = resolve_track_max_breadth(track.max, h_min, h_max, container_width_px);
    let (flex_factor, is_flexible) = match track.max {
        TrackMaxBreadth::Fr(fr) => (fr, true),
        _ => (0.0, false),
    };
    GridTrackEntry {
        base_px,
        limit_px,
        state_bits: if is_auto { GRID_TRACK_STATE_AUTO } else { 0 },
        flex_factor,
        is_flexible,
    }
}

fn resolve_track_min_breadth(
    breadth: TrackBreadth,
    h_min: f32,
    h_max: f32,
    container_width_px: f32,
) -> f32 {
    match breadth {
        TrackBreadth::Auto | TrackBreadth::MaxContent => h_max,
        TrackBreadth::Px(px) => px,
        TrackBreadth::Percent(p) => container_width_px * p * 0.01,
        TrackBreadth::MinContent => h_min,
    }
}

fn resolve_track_max_breadth(
    breadth: TrackMaxBreadth,
    h_min: f32,
    h_max: f32,
    container_width_px: f32,
) -> f32 {
    match breadth {
        TrackMaxBreadth::Auto | TrackMaxBreadth::Fr(_) => f32::INFINITY,
        TrackMaxBreadth::Px(px) => px,
        TrackMaxBreadth::Percent(p) => container_width_px * p * 0.01,
        TrackMaxBreadth::MinContent => h_min,
        TrackMaxBreadth::MaxContent => h_max,
    }
}

/// Sizes the explicit column tracks (D2): init, then maximize → fr expand →
/// stretch auto, operating in place on the track lane's column range.
fn size_column_tracks(
    scratch: &mut LayoutScratch,
    column_tracks: &[GridTrackSize; GRID_TRACK_LIST_CAPACITY],
    col_count: usize,
    col_h_min: &[f32; GRID_TRACK_LIST_CAPACITY],
    col_h_max: &[f32; GRID_TRACK_LIST_CAPACITY],
    container_width_px: f32,
    column_gap_px: f32,
) -> LaneRange {
    let range_start = scratch.tracks.begin_range();
    for col in 0..col_count {
        let entry = init_column_track(
            &column_tracks[col],
            col_h_min[col],
            col_h_max[col],
            container_width_px,
        );
        scratch.tracks.push(entry);
    }
    let range = scratch.tracks.finish_range(range_start);
    run_track_sizing_phases(scratch, range, container_width_px, column_gap_px);
    range
}

/// The D2 sizing phases (maximize → fr expand → stretch auto), in place. The
/// overflow edge (free ≤ 0) leaves every track at its base (fr gets its base).
fn run_track_sizing_phases(
    scratch: &mut LayoutScratch,
    range: LaneRange,
    container_width_px: f32,
    gap_px: f32,
) {
    let entries = scratch.tracks.slice_mut(range);
    let count = entries.len();
    if count == 0 {
        return;
    }
    let gap_total = gap_px * usize_to_f32(count.saturating_sub(1));
    let base_sum: f32 = entries.iter().map(|entry| entry.base_px).sum();
    let mut remaining = container_width_px - base_sum - gap_total;
    if remaining <= 0.0 {
        return;
    }

    // Maximize: distribute to tracks with FINITE growth limits (minmax with a
    // finite max), in track order. Auto and fr tracks are skipped (auto absorbs
    // in stretch-auto, fr in fr-expand).
    for entry in entries.iter_mut() {
        if entry.is_flexible || entry.state_bits & GRID_TRACK_STATE_AUTO != 0 {
            continue;
        }
        if entry.limit_px.is_finite() && entry.limit_px > entry.base_px {
            let take = (entry.limit_px - entry.base_px).min(remaining);
            entry.base_px += take;
            remaining -= take;
            if remaining <= 0.0 {
                break;
            }
        }
    }

    // Fr expand (CSS Grid 1 §7.2.4 "Find the Size of an fr"): the
    // iterate-and-freeze loop (Gecko `FindUsedFlexFraction`,
    // `nsGridContainerFrame.cpp`; Blink `FindFrSize`, which sorts by base/factor
    // and restarts). A flexible track whose content base exceeds its fr share is
    // treated as inflexible (frozen at its base) and removed from the pool; the
    // remaining space redistributes among the still-flexible tracks. The sum<1
    // cap (step 3) scales the leftover space by `min(Σfr, 1)` ONCE up front,
    // where Σfr is over ALL flexible tracks, frozen or not; the restart is per
    // remaining tracks. The loop terminates because each pass freezes ≥1 track
    // or breaks.
    if remaining > 0.0 {
        let flex_sum: f32 = entries
            .iter()
            .filter(|entry| entry.flex_factor > 0.0)
            .map(|entry| entry.flex_factor)
            .sum();
        if flex_sum > 0.0 {
            // The spec's leftover space is the free space PLUS the flexible
            // tracks' content bases (`remaining` already subtracted every
            // track's base); freezing a track charges its base back out of the
            // redistributable pool.
            let flex_base_sum: f32 = entries
                .iter()
                .filter(|entry| entry.flex_factor > 0.0)
                .map(|entry| entry.base_px)
                .sum();
            let mut free = (remaining + flex_base_sum) * flex_sum.min(1.0);
            // A frozen bitmap marks tracks removed from the pool (indexed by
            // slice position; `entries.len()` is bounded by the track-list
            // capacity, so a fixed bitmap is zero-allocation).
            let mut frozen = [false; GRID_TRACK_LIST_CAPACITY];
            loop {
                let mut factor_sum = 0.0f32;
                for (index, entry) in entries.iter().enumerate() {
                    if entry.flex_factor > 0.0 && !frozen[index] {
                        factor_sum += entry.flex_factor;
                    }
                }
                if factor_sum <= 0.0 {
                    break;
                }
                let fraction = free / factor_sum;
                let mut froze_any = false;
                for (index, entry) in entries.iter_mut().enumerate() {
                    if entry.flex_factor <= 0.0 || frozen[index] {
                        continue;
                    }
                    if entry.base_px > entry.flex_factor * fraction {
                        // Treated as inflexible: its base exceeds its fr share.
                        free -= entry.base_px;
                        frozen[index] = true;
                        froze_any = true;
                    }
                }
                if !froze_any {
                    for (index, entry) in entries.iter_mut().enumerate() {
                        if entry.flex_factor > 0.0 && !frozen[index] {
                            entry.base_px = entry.base_px.max(entry.flex_factor * fraction);
                        }
                    }
                    break;
                }
            }
        }
        // Recompute the leftover for the stretch-auto phase from the post-fr
        // base sizes (the loop sized in place; `remaining` is now stale).
        let sized_base_sum: f32 = entries.iter().map(|entry| entry.base_px).sum();
        remaining = container_width_px - sized_base_sum - gap_total;
    }

    // Stretch auto: auto tracks absorb any remaining free space equally.
    if remaining > 0.0 {
        let auto_count = entries
            .iter()
            .filter(|entry| entry.state_bits & GRID_TRACK_STATE_AUTO != 0)
            .count();
        if auto_count > 0 {
            let share = remaining / usize_to_f32(auto_count);
            for entry in entries.iter_mut() {
                if entry.state_bits & GRID_TRACK_STATE_AUTO != 0 {
                    entry.base_px += share;
                }
            }
        }
    }
}

/// Resolves each item's content width (stretch/definite) and row cross
/// contribution (refold for flagged-and-width-changed items), writing both into
/// the item lane, and assigns each item's auto-placed cell (D3).
#[allow(clippy::too_many_arguments)]
fn resolve_grid_items<R>(
    root_dom: &mut RootDom,
    tree_size: (f32, f32),
    child_context: &LayoutContext,
    order_range: LaneRange,
    col_count: usize,
    column_range: LaneRange,
    resolver: &mut R,
    metric: RootDomMetricName,
) -> Result<(), DomError>
where
    R: DomLayoutResolver + ?Sized,
{
    let order_len = usize::try_from(order_range.len).expect("order range len fits usize");
    let mut order_pos = 0usize;
    while order_pos < order_len {
        let col = u32::try_from(order_pos % col_count.max(1)).expect("col fits u32");
        let row = u32::try_from(order_pos / col_count.max(1)).expect("row fits u32");
        let (dom_index, view, horizontal_bp, vertical_bp, track_width_px) = {
            let order_entries = root_dom.layout_scratch.order.slice(order_range);
            let order_entry = order_entries[order_pos];
            let entry = root_dom
                .layout_scratch
                .grid_items
                .get(order_entry.dom_index);
            let track = root_dom.layout_scratch.tracks.get(column_range.start + col);
            (
                order_entry.dom_index,
                entry.view,
                entry.horizontal_border_padding_px,
                entry.vertical_border_padding_px,
                track.base_px,
            )
        };
        // The resolved content width: definite width, else the cell track width
        // minus insets (stretch main; ratio items stretch their main too).
        let resolved_content_width = {
            let (child_style, _) = resolved_node_style(root_dom, view.source.key(), child_context)?;
            child_style
                .width_px
                .unwrap_or_else(|| (track_width_px - horizontal_bp).max(0.0))
        };
        // Cross contribution: refold flagged + width-changed items; else the
        // measured border-box cross (content + insets).
        let cross_contribution = if view.main_influences_cross
            && !grid_sizes_match(resolved_content_width, view.content_main_px)
        {
            let refolded = refold_grid_item_at_resolved_width(
                root_dom,
                tree_size,
                child_context,
                view.source,
                resolved_content_width,
                resolver,
                metric,
            )?;
            root_dom.increment_metric(RootDomMetricName::NodesRefolded);
            refolded
        } else {
            view.content_cross_px + vertical_bp
        };
        let entry = root_dom.layout_scratch.grid_items.get_mut(dom_index);
        entry.row = row;
        entry.col = col;
        entry.resolved_content_width_px = resolved_content_width;
        entry.resolved_cross_contribution_px = cross_contribution;
        order_pos += 1;
    }
    Ok(())
}

fn grid_sizes_match(left_px: f32, right_px: f32) -> bool {
    (left_px - right_px).abs() <= f32::EPSILON
}

/// Re-measures one flagged grid item's subtree at its resolved content width
/// (the D4 refold): text re-shapes at the new width. Returns the item's
/// border-box cross (content + border/padding, no margins) for row sizing.
fn refold_grid_item_at_resolved_width<R>(
    root_dom: &mut RootDom,
    tree_size: (f32, f32),
    child_context: &LayoutContext,
    source: FlexItemSource,
    resolved_content_width_px: f32,
    resolver: &mut R,
    metric: RootDomMetricName,
) -> Result<f32, DomError>
where
    R: DomLayoutResolver + ?Sized,
{
    let key = source.key();
    let (child_style, child_inherited_style) = resolved_node_style(root_dom, key, child_context)?;
    let child_style = blockified_grid_item_style(child_style);
    let vertical_margins_px = child_style.sides.margin.top_px + child_style.sides.margin.bottom_px;
    let refold_context = LayoutContext {
        available_width_px: Some(resolved_content_width_px.max(0.0)),
        ..child_context.clone()
    };
    let child_flow = measure_tree_walker_node_without_layout_write(
        root_dom,
        tree_size,
        key,
        &refold_context,
        Some(resolved_content_width_px.max(0.0)),
        child_style,
        child_inherited_style,
        resolver,
        metric,
    )?;
    Ok(child_flow.height_px - vertical_margins_px)
}

/// The number of implicit rows for `order_count` items in `col_count` columns
/// (D3's `row = i / col_count`; 0 for an empty order lane).
fn implicit_row_count(order_range: LaneRange, col_count: usize) -> usize {
    if order_range.len == 0 {
        return 0;
    }
    let order_len = usize::try_from(order_range.len).expect("order range len fits usize");
    order_len.div_ceil(col_count.max(1))
}

/// Sizes the implicit rows (D4): each row = max over its items' resolved cross
/// contributions, resolved through `auto_rows` (content, or the specified
/// Length/Percent/minmax/Fr against the container height if definite).
#[allow(clippy::too_many_arguments)]
fn size_implicit_rows(
    scratch: &mut LayoutScratch,
    order_range: LaneRange,
    col_count: usize,
    row_count: usize,
    auto_rows: GridTrackSize,
    container_height_px: Option<f32>,
) -> LaneRange {
    let range_start = scratch.tracks.begin_range();
    for row in 0..row_count {
        let mut content_cross = 0.0f32;
        for col in 0..col_count {
            let index = row * col_count + col;
            if index >= usize::try_from(order_range.len).expect("order len fits usize") {
                break;
            }
            let order_entries = scratch.order.slice(order_range);
            let order_entry = order_entries[index];
            let entry = scratch.grid_items.get(order_entry.dom_index);
            content_cross = content_cross.max(entry.resolved_cross_contribution_px);
        }
        let height = resolve_implicit_row_height(auto_rows, content_cross, container_height_px);
        scratch.tracks.push(GridTrackEntry {
            base_px: height,
            limit_px: height,
            state_bits: 0,
            flex_factor: 0.0,
            is_flexible: false,
        });
    }
    scratch.tracks.finish_range(range_start)
}

/// Resolves one implicit row's height from `auto_rows`: content, or the
/// specified track size resolved against the container height (if definite),
/// else content (D4).
fn resolve_implicit_row_height(
    auto_rows: GridTrackSize,
    content_cross_px: f32,
    container_height_px: Option<f32>,
) -> f32 {
    let min = match auto_rows.min {
        TrackBreadth::Auto => 0.0,
        TrackBreadth::Px(px) => px,
        TrackBreadth::Percent(p) => container_height_px.map_or(0.0, |h| h * p * 0.01),
        TrackBreadth::MinContent | TrackBreadth::MaxContent => content_cross_px,
    };
    let max = match auto_rows.max {
        TrackMaxBreadth::Auto | TrackMaxBreadth::Fr(_) => f32::INFINITY,
        TrackMaxBreadth::Px(px) => px,
        TrackMaxBreadth::Percent(p) => {
            container_height_px.map_or(content_cross_px, |h| h * p * 0.01)
        }
        TrackMaxBreadth::MinContent | TrackMaxBreadth::MaxContent => content_cross_px,
    };
    content_cross_px.clamp(min, max)
}

/// The container's resolved content height: Σ row heights + row gaps.
fn total_row_height(scratch: &LayoutScratch, row_range: LaneRange, row_gap_px: f32) -> f32 {
    let rows = scratch.tracks.slice(row_range);
    let sum: f32 = rows.iter().map(|entry| entry.base_px).sum();
    let gap_total = row_gap_px * usize_to_f32(rows.len().saturating_sub(1));
    (sum + gap_total).max(0.0)
}

/// Places each item into its cell (D5): stretch to cell with the definite-size
/// and replaced-ratio exemptions, absorb per-axis auto margins, and relayout the
/// child subtree at the resolved cell rect. Returns the container content height.
#[allow(clippy::too_many_arguments)]
fn place_grid_children<R>(
    root_dom: &mut RootDom,
    tree_size: (f32, f32),
    child_context: &LayoutContext,
    solve: GridContainerSolve,
    column_gap_px: f32,
    row_gap_px: f32,
    resolver: &mut R,
) -> Result<f32, DomError>
where
    R: DomLayoutResolver + ?Sized,
{
    let item_count = usize::try_from(solve.item_range.len).expect("item range len fits usize");
    let mut placed_item_count = 0usize;
    for order_pos in 0..item_count {
        let (dom_index, cell_left, cell_top, track_width, row_height) = {
            let order_entries = root_dom.layout_scratch.order.slice(solve.order_range);
            let order_entry = order_entries[order_pos];
            let entry = root_dom
                .layout_scratch
                .grid_items
                .get(order_entry.dom_index);
            (
                order_entry.dom_index,
                track_offset(
                    &root_dom.layout_scratch,
                    solve.column_range,
                    entry.col,
                    column_gap_px,
                ),
                track_offset(
                    &root_dom.layout_scratch,
                    solve.row_range,
                    entry.row,
                    row_gap_px,
                ),
                root_dom
                    .layout_scratch
                    .tracks
                    .get(solve.column_range.start + entry.col)
                    .base_px,
                root_dom
                    .layout_scratch
                    .tracks
                    .get(solve.row_range.start + entry.row)
                    .base_px,
            )
        };
        let entry = *root_dom.layout_scratch.grid_items.get(dom_index);
        let (child_style, _) =
            resolved_node_style(root_dom, entry.view.source.key(), child_context)?;
        let placement = resolve_grid_child_placement(
            child_context,
            cell_left,
            cell_top,
            track_width,
            row_height,
            &child_style,
            &entry,
        );
        let GridChildPlacement {
            context: placed_context,
            absorbed_left_px,
            absorbed_right_px,
            absorbed_top_px,
            absorbed_bottom_px,
        } = placement;
        relayout_tree_walker_node(
            root_dom,
            tree_size,
            entry.view.source.key(),
            placed_context,
            resolver,
        )?;
        root_dom.increment_metric(RootDomMetricName::GridNodesPlaced);
        write_grid_item_absorbed_auto_margins(
            root_dom,
            entry.view.source,
            absorbed_left_px,
            absorbed_right_px,
            absorbed_top_px,
            absorbed_bottom_px,
        )?;
        placed_item_count += 1;
    }
    assert_eq!(
        placed_item_count, item_count,
        "active grid placement should write each resolved child exactly once"
    );
    Ok(solve.content_height_px)
}

/// The sum of the first `index` track sizes plus `index` gaps (the cell's
/// content-box offset along an axis).
fn track_offset(scratch: &LayoutScratch, range: LaneRange, index: u32, gap_px: f32) -> f32 {
    let tracks = scratch.tracks.slice(range);
    let index_usize = usize::try_from(index).expect("track index fits usize");
    let mut offset = 0.0f32;
    for track in tracks.iter().take(index_usize) {
        offset += track.base_px;
    }
    offset + gap_px * usize_to_f32(index_usize)
}

/// Resolves one grid item's placement: the content origin (cell top-left +
/// absorbed auto start margins), the stretch/definite/ratio content overrides,
/// and the four absorbed auto margins.
fn resolve_grid_child_placement(
    child_context: &LayoutContext,
    cell_left_px: f32,
    cell_top_px: f32,
    track_width_px: f32,
    row_height_px: f32,
    child_style: &NodeStyle,
    entry: &GridItemLaneEntry,
) -> GridChildPlacement {
    let view = &entry.view;
    let horizontal_bp = entry.horizontal_border_padding_px;
    let vertical_bp = entry.vertical_border_padding_px;
    let horizontal_auto = view.auto_margins & (MARGIN_LEFT_AUTO | MARGIN_RIGHT_AUTO) != 0;
    let vertical_auto = view.auto_margins & (MARGIN_TOP_AUTO | MARGIN_BOTTOM_AUTO) != 0;

    // Resolved content width: definite → definite; auto horizontal margins →
    // fit-content (natural, not stretched); else stretch to the track width.
    let resolved_content_width = match child_style.width_px {
        Some(definite) => definite,
        None if horizontal_auto => view.content_main_px,
        None => (track_width_px - horizontal_bp).max(0.0),
    };

    // Content-height override: definite → None (style owns it); replaced-ratio
    // → None (ratio-derived from width); auto vertical margins → None (natural,
    // not stretched); else stretch to the row height.
    let content_height_override =
        if child_style.height_px.is_some() || view.replaced_ratio || vertical_auto {
            None
        } else {
            Some((row_height_px - vertical_bp).max(0.0))
        };

    // Absorbed auto margins (CSS Box Alignment §8.1): free = cell − border-box
    // − non-auto margins, distributed equally among that axis's auto margins.
    let border_box_width = resolved_content_width + horizontal_bp;
    let (absorbed_left_px, absorbed_right_px) = absorbed_auto_margins(
        view,
        horizontal_auto,
        track_width_px - border_box_width - view.margins[1] - view.margins[3],
        MARGIN_LEFT_AUTO,
        MARGIN_RIGHT_AUTO,
    );
    let resolved_content_height = child_style.height_px.unwrap_or(view.content_cross_px);
    let border_box_height = resolved_content_height + vertical_bp;
    let (absorbed_top_px, absorbed_bottom_px) = absorbed_auto_margins(
        view,
        vertical_auto,
        row_height_px - border_box_height - view.margins[0] - view.margins[2],
        MARGIN_TOP_AUTO,
        MARGIN_BOTTOM_AUTO,
    );

    let context = LayoutContext {
        flow_left_px: child_context.flow_left_px + cell_left_px + absorbed_left_px,
        flow_top_px: child_context.flow_top_px + cell_top_px + absorbed_top_px,
        available_width_px: child_context.available_width_px,
        auto_margin_reference_width_px: Some(resolved_content_width),
        node_content_width_override_px: Some(resolved_content_width.max(0.0)),
        node_content_height_override_px: content_height_override,
        ..child_context.clone()
    };

    GridChildPlacement {
        context,
        absorbed_left_px,
        absorbed_right_px,
        absorbed_top_px,
        absorbed_bottom_px,
    }
}

/// Distributes one axis's free space equally among that axis's auto margins,
/// returning the absorbed `(start, end)` amounts (physical left/top for start,
/// right/bottom for end). A non-auto side absorbs nothing.
fn absorbed_auto_margins(
    view: &GridItemView,
    has_auto: bool,
    free_space_px: f32,
    start_bit: u8,
    end_bit: u8,
) -> (f32, f32) {
    if !has_auto || free_space_px <= 0.0 {
        return (0.0, 0.0);
    }
    let start_auto = view.auto_margins & start_bit != 0;
    let end_auto = view.auto_margins & end_bit != 0;
    let auto_count = usize::from(start_auto) + usize::from(end_auto);
    if auto_count == 0 {
        return (0.0, 0.0);
    }
    let share = free_space_px / usize_to_f32(auto_count);
    (
        if start_auto { share } else { 0.0 },
        if end_auto { share } else { 0.0 },
    )
}

/// Writes a grid item's absorbed auto margins into its retained box model's
/// MARGIN edges (CSS Box Alignment §8.1) for paint/hit-test. Mirrors the flex
/// `write_flex_item_absorbed_auto_margins`; the border-box origin is NOT shifted
/// here (the placement context already applied the absorbed start margins).
fn write_grid_item_absorbed_auto_margins(
    root_dom: &mut RootDom,
    source: FlexItemSource,
    absorbed_left: f32,
    absorbed_right: f32,
    absorbed_top: f32,
    absorbed_bottom: f32,
) -> Result<(), DomError> {
    let FlexItemSource::BlockItem(key) = source else {
        return Ok(());
    };
    if absorbed_left == 0.0
        && absorbed_right == 0.0
        && absorbed_top == 0.0
        && absorbed_bottom == 0.0
    {
        return Ok(());
    }
    let node = root_dom
        .nodes
        .get_mut(key)
        .ok_or(DomError::InvalidNodeKey)?;
    match node {
        DomNode::Document(document) => {
            document.base.box_model.margin.left_px += absorbed_left;
            document.base.box_model.margin.right_px += absorbed_right;
            document.base.box_model.margin.top_px += absorbed_top;
            document.base.box_model.margin.bottom_px += absorbed_bottom;
        }
        DomNode::DocumentFragment(fragment) => {
            fragment.base.box_model.margin.left_px += absorbed_left;
            fragment.base.box_model.margin.right_px += absorbed_right;
            fragment.base.box_model.margin.top_px += absorbed_top;
            fragment.base.box_model.margin.bottom_px += absorbed_bottom;
        }
        DomNode::Element(element) => {
            element.base.box_model.margin.left_px += absorbed_left;
            element.base.box_model.margin.right_px += absorbed_right;
            element.base.box_model.margin.top_px += absorbed_top;
            element.base.box_model.margin.bottom_px += absorbed_bottom;
        }
        DomNode::Text(text) => {
            text.base.box_model.margin.left_px += absorbed_left;
            text.base.box_model.margin.right_px += absorbed_right;
            text.base.box_model.margin.top_px += absorbed_top;
            text.base.box_model.margin.bottom_px += absorbed_bottom;
        }
    }
    Ok(())
}

/// Lays out each EXCLUDED abs-pos/fixed child at the T1 static position: the
/// container's padding-box top-left (auto lines fall back to padding-box edges,
/// CSS Grid §8.4), positioned per CSS 2.1 §10.3.7/§10.6.4 against the padding
/// box (D6).
#[allow(clippy::too_many_arguments)]
fn place_excluded_abs_grid_children<R>(
    root_dom: &mut RootDom,
    tree_size: (f32, f32),
    container_key: DomNodeSlotKey,
    entered: &EnteredNodeLayout,
    child_context: &LayoutContext,
    content_width_px: f32,
    content_height_px: f32,
    resolver: &mut R,
) -> Result<(), DomError>
where
    R: DomLayoutResolver + ?Sized,
{
    let padding_left_px = entered.absolute_left_px + entered.style.sides.border.left_px;
    let padding_top_px = entered.absolute_top_px + entered.style.sides.border.top_px;
    let padding_box_width_px = content_width_px
        + entered.style.sides.padding.left_px
        + entered.style.sides.padding.right_px;
    let padding_box_height_px = content_height_px
        + entered.style.sides.padding.top_px
        + entered.style.sides.padding.bottom_px;

    let mut child_key = first_child_key(root_dom, container_key)?;
    while let Some(current_child_key) = child_key {
        let (is_abs_fixed, next_child_key) = {
            let current_node = root_dom
                .nodes
                .get(current_child_key)
                .ok_or(DomError::InvalidNodeKey)?;
            (
                matches!(
                    presented_position(current_node),
                    PositionMode::Absolute | PositionMode::Fixed
                ),
                current_node.next_sibling_key_for_store(),
            )
        };
        if is_abs_fixed {
            let abs_context = LayoutContext {
                containing_layer_absolute_left_px: padding_left_px,
                containing_layer_absolute_top_px: padding_top_px,
                containing_layer_width_px: padding_box_width_px,
                containing_layer_height_px: padding_box_height_px,
                flow_left_px: 0.0,
                flow_top_px: 0.0,
                ..child_context.clone()
            };
            relayout_tree_walker_node(
                root_dom,
                tree_size,
                current_child_key,
                abs_context,
                resolver,
            )?;
            // The static position is the padding-box origin (auto lines fall
            // back to padding-box edges); the content origin adds the padding.
            let content_left_px = padding_left_px + entered.style.sides.padding.left_px;
            let content_top_px = padding_top_px + entered.style.sides.padding.top_px;
            write_node_border_box_position(
                root_dom,
                current_child_key,
                content_left_px,
                content_top_px,
            )?;
        }
        child_key = next_child_key;
    }
    Ok(())
}

/// The D7 container intrinsic inline sizes (`h_min`/`h_max` for the parent's
/// consumption). `percent_basis` resolves percent tracks: `Some(width)` against
/// the definite container width, `None` content-based (the inline-grid
/// shrink-to-fit case, correction 4).
fn grid_container_intrinsic_inline_sizes(
    column_tracks: &[GridTrackSize; GRID_TRACK_LIST_CAPACITY],
    col_count: usize,
    col_h_min: &[f32; GRID_TRACK_LIST_CAPACITY],
    col_h_max: &[f32; GRID_TRACK_LIST_CAPACITY],
    column_gap_px: f32,
    percent_basis: Option<f32>,
) -> (f32, f32) {
    let h_min = grid_intrinsic_inline_size_impl(
        column_tracks,
        col_count,
        col_h_min,
        col_h_max,
        column_gap_px,
        false,
        percent_basis,
    );
    let h_max = grid_intrinsic_inline_size_impl(
        column_tracks,
        col_count,
        col_h_min,
        col_h_max,
        column_gap_px,
        true,
        percent_basis,
    );
    (h_min, h_max)
}

/// One pass of the D7 intrinsic fold: min-side (`use_max = false`) or max-side
/// (`use_max = true`), per the track kinds.
#[allow(clippy::too_many_arguments)]
fn grid_intrinsic_inline_size_impl(
    column_tracks: &[GridTrackSize; GRID_TRACK_LIST_CAPACITY],
    col_count: usize,
    col_h_min: &[f32; GRID_TRACK_LIST_CAPACITY],
    col_h_max: &[f32; GRID_TRACK_LIST_CAPACITY],
    column_gap_px: f32,
    use_max: bool,
    percent_basis: Option<f32>,
) -> f32 {
    if col_count == 0 {
        return 0.0;
    }
    let mut sum = 0.0f32;
    for col in 0..col_count {
        let track = &column_tracks[col];
        let contribution = if use_max {
            intrinsic_max_contribution(track, col_h_max[col], col_h_min[col], percent_basis)
        } else {
            intrinsic_min_contribution(track, col_h_min[col], col_h_max[col], percent_basis)
        };
        sum += contribution;
    }
    sum + column_gap_px * usize_to_f32(col_count.saturating_sub(1))
}

fn intrinsic_min_contribution(
    track: &GridTrackSize,
    h_min: f32,
    h_max: f32,
    percent_basis: Option<f32>,
) -> f32 {
    match track.min {
        TrackBreadth::Auto | TrackBreadth::MinContent => h_min,
        TrackBreadth::Px(px) => px,
        TrackBreadth::Percent(p) => percent_basis.map_or(h_min, |w| w * p * 0.01),
        TrackBreadth::MaxContent => h_max,
    }
}

fn intrinsic_max_contribution(
    track: &GridTrackSize,
    h_max: f32,
    h_min: f32,
    percent_basis: Option<f32>,
) -> f32 {
    match track.max {
        TrackMaxBreadth::Auto | TrackMaxBreadth::MaxContent | TrackMaxBreadth::Fr(_) => h_max,
        TrackMaxBreadth::Px(px) => px,
        TrackMaxBreadth::Percent(p) => percent_basis.map_or(h_max, |w| w * p * 0.01),
        TrackMaxBreadth::MinContent => h_min,
    }
}

/// The grid measure path for a NESTED grid container (as a flex item or block
/// child): the measure fold + column/row sizing without a layout write, writing
/// the container's record with the D7 intrinsic and the resolved content sizes.
#[allow(clippy::too_many_arguments)]
pub(super) fn measure_grid_node_without_layout_write<R>(
    root_dom: &mut RootDom,
    tree_size: (f32, f32),
    key: DomNodeSlotKey,
    context: &LayoutContext,
    _wrap_width_override: Option<f32>,
    style: NodeStyle,
    inherited_style: InheritedStyleContext,
    resolver: &mut R,
    metric: RootDomMetricName,
) -> Result<FlowSize, DomError>
where
    R: DomLayoutResolver + ?Sized,
{
    let saved_grid_items_len = root_dom.layout_scratch.grid_items.len();
    let saved_order_len = root_dom.layout_scratch.order.len();
    let saved_tracks_len = root_dom.layout_scratch.tracks.len();

    let (grid_group, column_gap_px, row_gap_px) = {
        let container_node = root_dom.nodes.get(key).ok_or(DomError::InvalidNodeKey)?;
        let gap = container_node.computed_flex().gap;
        (container_node.stored_computed_groups().grid, gap[1], gap[0])
    };
    let mut column_tracks = empty_track_array();
    let col_count = resolve_columns(root_dom, grid_group.template_columns, &mut column_tracks);

    let entered = enter_node(
        root_dom,
        tree_size,
        key,
        context.clone(),
        style,
        inherited_style,
        resolver,
    )?;
    let child_context = child_layout_context(context.clone(), entered.clone());

    let (_item_range, order_range) =
        build_grid_item_formation(root_dom, tree_size, key, &child_context, resolver, metric)?;
    let (col_h_min, col_h_max) = column_contributions(root_dom, order_range, col_count);

    let container_content_width = grid_container_content_width(
        &entered,
        &column_tracks,
        col_count,
        &col_h_min,
        &col_h_max,
        column_gap_px,
    );

    let column_range = size_column_tracks(
        &mut root_dom.layout_scratch,
        &column_tracks,
        col_count,
        &col_h_min,
        &col_h_max,
        container_content_width,
        column_gap_px,
    );

    resolve_grid_items(
        root_dom,
        tree_size,
        &child_context,
        order_range,
        col_count,
        column_range,
        resolver,
        metric,
    )?;

    let row_count = implicit_row_count(order_range, col_count);
    let row_range = size_implicit_rows(
        &mut root_dom.layout_scratch,
        order_range,
        col_count,
        row_count,
        grid_group.auto_rows,
        entered.style.height_px,
    );
    let content_height_px = total_row_height(&root_dom.layout_scratch, row_range, row_gap_px);

    let (h_min, h_max) = grid_container_intrinsic_inline_sizes(
        &column_tracks,
        col_count,
        &col_h_min,
        &col_h_max,
        column_gap_px,
        Some(container_content_width),
    );

    let flow = measure_grid_exited_flow_size(
        root_dom,
        key,
        &entered,
        container_content_width,
        (h_min, h_max),
        content_height_px,
        metric,
    )?;

    root_dom
        .layout_scratch
        .grid_items
        .truncate(saved_grid_items_len);
    root_dom.layout_scratch.order.truncate(saved_order_len);
    root_dom.layout_scratch.tracks.truncate(saved_tracks_len);
    Ok(flow)
}

/// The grid measure epilogue: writes the container's record with the D7
/// intrinsic inline sizes and the resolved content size, counts one
/// `GridNodesMeasured`, and returns the folded `FlowSize`.
fn measure_grid_exited_flow_size(
    root_dom: &mut RootDom,
    key: DomNodeSlotKey,
    entered: &EnteredNodeLayout,
    auto_content_width_px: f32,
    auto_content_width_contribution_px: (f32, f32),
    auto_content_height_px: f32,
    metric: RootDomMetricName,
) -> Result<FlowSize, DomError> {
    let (h_min_content_px, h_max_content_px) = auto_content_width_contribution_px;
    let content_size = resolved_content_size_px(
        entered_content_size_property_style(entered),
        entered.img_intrinsic_aspect_ratio,
        None,
        auto_content_width_px,
        auto_content_height_px,
    );
    let content_height_px = content_size.viewport_height_px;
    let mut next_record =
        compute_node_measurement(entered, (h_min_content_px, h_max_content_px), content_size);
    next_record.measure_pass = root_dom.layout_scratch.layout_pass();
    next_record.content_height_measured_at_width_px = auto_content_width_px;
    if !entered.is_text_node && node_has_main_influences_cross_child(root_dom, key)? {
        next_record.flags = next_record.flags.with_main_influences_cross();
    }
    root_dom.increment_metric(metric);
    let _ = write_measurement_if_changed(&mut root_dom.nodes, key, next_record)?;
    let box_model = DomBoxModel {
        border_box: DomRect {
            left_px: 0.0,
            top_px: 0.0,
            width_px: border_box_width(entered.style.sides, content_size.width_px),
            height_px: border_box_height(entered.style.sides, content_height_px),
        },
        margin: entered.box_model.margin,
        border: entered.box_model.border,
        padding: entered.box_model.padding,
    };
    Ok(layout_flow_size(
        entered.style,
        box_model,
        h_min_content_px,
        h_max_content_px,
    ))
}

#[cfg(test)]
#[allow(clippy::float_cmp)] // track sizing arithmetic asserts exact literals
mod tests {
    use super::*;

    fn track(min: TrackBreadth, max: TrackMaxBreadth) -> GridTrackSize {
        GridTrackSize { min, max }
    }

    fn px(min: f32, max: f32) -> GridTrackSize {
        track(TrackBreadth::Px(min), TrackMaxBreadth::Px(max))
    }

    fn fr(f: f32) -> GridTrackSize {
        track(TrackBreadth::Auto, TrackMaxBreadth::Fr(f))
    }

    fn size_tracks(tracks: &[GridTrackSize], container_width: f32, gap: f32) -> Vec<f32> {
        size_tracks_with_contributions(tracks, &[], &[], container_width, gap)
    }

    /// `size_tracks` with per-column `h_min`/`h_max` item contributions (the
    /// fr tracks' base is their `h_max`; the freeze pins need non-zero bases).
    fn size_tracks_with_contributions(
        tracks: &[GridTrackSize],
        h_min: &[f32],
        h_max: &[f32],
        container_width: f32,
        gap: f32,
    ) -> Vec<f32> {
        let mut scratch = LayoutScratch::default();
        let mut column_tracks = empty_track_array();
        for (slot, t) in column_tracks.iter_mut().zip(tracks.iter()) {
            *slot = *t;
        }
        let mut min = [0.0f32; GRID_TRACK_LIST_CAPACITY];
        let mut max = [0.0f32; GRID_TRACK_LIST_CAPACITY];
        min[..h_min.len()].copy_from_slice(h_min);
        max[..h_max.len()].copy_from_slice(h_max);
        let range = size_column_tracks(
            &mut scratch,
            &column_tracks,
            tracks.len(),
            &min,
            &max,
            container_width,
            gap,
        );
        scratch
            .tracks
            .slice(range)
            .iter()
            .map(|e| e.base_px)
            .collect()
    }

    #[test]
    fn fixed_and_fr_mix_distributes_after_fixed_base() {
        // `100px 1fr 2fr` in 300px → 100 / 66.67 / 133.33.
        let tracks = [px(100.0, 100.0), fr(1.0), fr(2.0)];
        let sizes = size_tracks(&tracks, 300.0, 0.0);
        assert_eq!(sizes[0], 100.0);
        assert!((sizes[1] - 200.0 / 3.0).abs() < 0.01);
        assert!((sizes[2] - 400.0 / 3.0).abs() < 0.01);
    }

    #[test]
    fn fr_sum_below_one_scales_free_space() {
        // `0.25fr 0.25fr` in 400px (Σ=0.5 < 1) → 100 / 100 (total 200, NOT 400).
        let tracks = [fr(0.25), fr(0.25)];
        let sizes = size_tracks(&tracks, 400.0, 0.0);
        assert!((sizes[0] - 100.0).abs() < 0.01);
        assert!((sizes[1] - 100.0).abs() < 0.01);
    }

    #[test]
    fn minmax_clamps_to_its_limit_and_auto_absorbs_the_rest() {
        // `minmax(100px, 200px) auto` in 300px → 200 / 100.
        let tracks = [
            track(TrackBreadth::Px(100.0), TrackMaxBreadth::Px(200.0)),
            track(TrackBreadth::Auto, TrackMaxBreadth::Auto),
        ];
        let sizes = size_tracks(&tracks, 300.0, 0.0);
        assert_eq!(sizes[0], 200.0);
        assert_eq!(sizes[1], 100.0);
    }

    #[test]
    fn overflow_keeps_tracks_at_their_bases() {
        // `200px 200px 1fr` in 300px → 200 / 200 / 0 (fr gets its content base).
        let tracks = [px(200.0, 200.0), px(200.0, 200.0), fr(1.0)];
        let sizes = size_tracks(&tracks, 300.0, 0.0);
        assert_eq!(sizes[0], 200.0);
        assert_eq!(sizes[1], 200.0);
        assert_eq!(sizes[2], 0.0);
    }

    #[test]
    fn gap_is_subtracted_before_distribution() {
        // `1fr 1fr 1fr` in 300px with 10px column gaps → (300 − 20) / 3 = 93.33.
        let tracks = [fr(1.0), fr(1.0), fr(1.0)];
        let sizes = size_tracks(&tracks, 300.0, 10.0);
        for size in sizes {
            assert!((size - 280.0 / 3.0).abs() < 0.01);
        }
    }

    #[test]
    fn fr_expand_freezes_over_base_tracks_and_redistributes() {
        // CSS Grid §7.2.4: `1fr 1fr` in 400px with a 300px col-0 base → 300/100.
        // The old closed form left 50px unfilled (300/50).
        let tracks = [fr(1.0), fr(1.0)];
        let sizes = size_tracks_with_contributions(&tracks, &[0.0, 0.0], &[300.0, 0.0], 400.0, 0.0);
        assert_eq!(sizes[0], 300.0);
        assert_eq!(sizes[1], 100.0);
    }

    #[test]
    fn fr_expand_partial_freeze_redistributes_among_the_rest() {
        // `1fr 1fr 1fr` in 400px, col-0 base 200 → 200 / 100 / 100.
        let tracks = [fr(1.0), fr(1.0), fr(1.0)];
        let sizes = size_tracks_with_contributions(
            &tracks,
            &[0.0, 0.0, 0.0],
            &[200.0, 0.0, 0.0],
            400.0,
            0.0,
        );
        assert_eq!(sizes[0], 200.0);
        assert_eq!(sizes[1], 100.0);
        assert_eq!(sizes[2], 100.0);
    }
}
