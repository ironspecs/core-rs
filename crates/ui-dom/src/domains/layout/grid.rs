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

use crate::domains::style::computed::{
    AlignItemsKw, BoxLength, ComputedGridGroup, GridAutoFlowKw, GridLine, GridTrackSize,
    JustifyItemsKw, JustifySelfKw, TrackBreadth, TrackMaxBreadth,
};
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
use super::grid_cell_map::GridCellMap;
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
    /// The authored `grid-column-start` line (item side, D1).
    pub(crate) column_start: GridLine,
    /// The authored `grid-column-end` line (item side, D1).
    pub(crate) column_end: GridLine,
    /// The authored `grid-row-start` line (item side, D1).
    pub(crate) row_start: GridLine,
    /// The authored `grid-row-end` line (item side, D1).
    pub(crate) row_end: GridLine,
    /// The item's `justify-self` (inline axis, D5; `Auto` inherits the
    /// container's `justify-items`).
    pub(crate) justify_self: JustifySelfKw,
    /// The item's `align-self` (block axis, D5; `None` = `auto` — inherit the
    /// container's `align-items`).
    pub(crate) align_self: Option<AlignItemsKw>,
    /// The resolved column span (written by §8.5 placement; 1 until then).
    pub(crate) col_span: u32,
    /// The resolved row span (written by §8.5 placement; 1 until then).
    pub(crate) row_span: u32,
}

/// The resolved inline-axis (horizontal) cell alignment for one grid item (D5).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GridInlineAlignment {
    Stretch,
    Start,
    End,
    Center,
}

/// The resolved block-axis (vertical) cell alignment for one grid item (D5).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GridBlockAxisAlignment {
    Stretch,
    Start,
    End,
    Center,
}

/// The resolved placement for one axis (D2/D4): a definite `(start, end)` line
/// pair (0-based, untranslated), or an auto-placement span.
#[derive(Clone, Copy, Debug, PartialEq)]
enum ResolvedAxis {
    Definite(i32, i32),
    Auto(u32),
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

/// The §8.5 placement resolution: the implicit grid size plus the container
/// offsets (implicit tracks added left/top of the explicit grid by negative
/// lines, D2).
#[derive(Clone, Copy, Debug, PartialEq)]
struct GridPlacement {
    total_col_count: usize,
    total_row_count: usize,
    col_offset: usize,
    row_offset: usize,
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
    let (grid_group, column_gap_px, row_gap_px, container_align_items) = {
        let container_node = root_dom.nodes.get(key).ok_or(DomError::InvalidNodeKey)?;
        let flex = container_node.computed_flex();
        (
            container_node.stored_computed_groups().grid,
            flex.gap[1],
            flex.gap[0],
            flex.align_items,
        )
    };
    let mut column_tracks = empty_track_array();
    let col_count = resolve_columns(root_dom, grid_group.template_columns, &mut column_tracks);
    let mut row_tracks = empty_track_array();
    let explicit_row_count = resolve_rows(root_dom, grid_group.template_rows, &mut row_tracks);

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

    // 2. Placement resolution (D2 + D4): resolve each item's cell and spans via
    // the CellMap, writing cell starts and spans into the item lane / view.
    let placement = resolve_grid_placement(
        &mut root_dom.layout_scratch,
        order_range,
        col_count,
        explicit_row_count,
        grid_group.auto_flow,
    );

    // 3. Per-column contributions (max h_min / h_max per explicit column, from
    // the resolved placement + spans).
    let (col_h_min, col_h_max) = column_contributions(
        &root_dom.layout_scratch,
        item_range,
        col_count,
        placement.col_offset,
    );

    // 4. Container content width (block → CB width; InlineGrid → D7 intrinsic).
    let container_content_width = grid_container_content_width(
        &entered,
        &column_tracks,
        col_count,
        &col_h_min,
        &col_h_max,
        column_gap_px,
    );

    // 5. Column track sizing (explicit columns by the phase algorithm, implicit
    // columns by `auto_columns`).
    let column_range = size_column_tracks(
        &mut root_dom.layout_scratch,
        &column_tracks,
        col_count,
        placement.total_col_count,
        placement.col_offset,
        item_range,
        &col_h_min,
        &col_h_max,
        container_content_width,
        column_gap_px,
        grid_group.auto_columns,
    );

    // 6. Item resolution (resolved width across the span + cross contribution,
    // refold).
    resolve_grid_items(
        root_dom,
        tree_size,
        &child_context,
        order_range,
        column_range,
        column_gap_px,
        &grid_group,
        resolver,
        RootDomMetricName::GridNodesMeasured,
    )?;

    // 7. Row track sizing (explicit rows by the phase algorithm, implicit rows
    // by `auto_rows`).
    let row_range = size_row_tracks(
        &mut root_dom.layout_scratch,
        item_range,
        &row_tracks,
        explicit_row_count,
        placement.total_row_count,
        placement.row_offset,
        grid_group.auto_rows,
        entered.style.height_px,
    );
    let content_height_px = total_row_height(&root_dom.layout_scratch, row_range, row_gap_px);

    // 8. Placement + abs-pos pass.
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
        &grid_group,
        container_align_items,
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
        column_range,
        row_range,
        placement.col_offset,
        placement.row_offset,
        column_gap_px,
        row_gap_px,
        col_count,
        explicit_row_count,
        resolver,
    )?;

    // 9. Container intrinsic contribution (D7).
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

/// Copies the interned explicit ROW list into the caller's stack array and
/// returns the real row count (0 for the empty template — all rows implicit,
/// sized by `grid-auto-rows`, D3).
fn resolve_rows(
    root_dom: &RootDom,
    key: GridTrackListKey,
    row_tracks: &mut [GridTrackSize; GRID_TRACK_LIST_CAPACITY],
) -> usize {
    let mut count = 0;
    if let Some(list) = root_dom.resolve_inline_track_list(key) {
        for (slot, track) in row_tracks.iter_mut().zip(list.iter()) {
            *slot = *track;
            count += 1;
        }
    }
    count
}

/// Resolves a definite integer line to a 0-based line index against the
/// explicit grid (D2): `k > 0` → `k - 1`; `k < 0` → `explicit_count + k + 1`
/// (zero never reaches here — the grammar drops it). Negatives may produce
/// negative 0-based indices; the container-wide offset translates them (D2).
fn resolve_grid_line(k: i32, explicit_count: usize) -> i32 {
    let count = i32::try_from(explicit_count).expect("grid track count fits i32");
    if k > 0 { k - 1 } else { count + k + 1 }
}

/// §8.3.1 + §8.4: a definite `(start, end)` line range is normalized — swapped
/// when start > end, expanded to span 1 when equal.
fn normalize_line_range(start: i32, end: i32) -> (i32, i32) {
    match start.cmp(&end) {
        std::cmp::Ordering::Greater => (end, start),
        std::cmp::Ordering::Equal => (start, start + 1),
        std::cmp::Ordering::Less => (start, end),
    }
}

/// Resolves one axis's `(start, end)` `GridLine`s to a `ResolvedAxis` (D2):
/// definite when at least one side is a `Line` (a lone `Auto` side defaults to
/// span 1; a `Span` side resolves from the definite line), auto with the span
/// otherwise (`span N / auto`, `auto / span N`, or `span N / span M` — the
/// second span is ignored per §8.3.1).
#[allow(clippy::unnested_or_patterns)] // the three span-only forms share one arm
fn resolve_axis_range(start: GridLine, end: GridLine, explicit_count: usize) -> ResolvedAxis {
    match (start, end) {
        (GridLine::Line(s), GridLine::Line(e)) => {
            let range = normalize_line_range(
                resolve_grid_line(s, explicit_count),
                resolve_grid_line(e, explicit_count),
            );
            ResolvedAxis::Definite(range.0, range.1)
        }
        (GridLine::Line(s), GridLine::Auto) => {
            let line = resolve_grid_line(s, explicit_count);
            ResolvedAxis::Definite(line, line + 1)
        }
        (GridLine::Line(s), GridLine::Span(n)) => {
            let line = resolve_grid_line(s, explicit_count);
            ResolvedAxis::Definite(line, line + i32::try_from(n).expect("span fits i32"))
        }
        (GridLine::Span(n), GridLine::Line(e)) => {
            let line = resolve_grid_line(e, explicit_count);
            ResolvedAxis::Definite(line - i32::try_from(n).expect("span fits i32"), line)
        }
        (GridLine::Auto, GridLine::Line(e)) => {
            let line = resolve_grid_line(e, explicit_count);
            ResolvedAxis::Definite(line - 1, line)
        }
        (GridLine::Span(n), GridLine::Auto)
        | (GridLine::Auto, GridLine::Span(n))
        | (GridLine::Span(n), GridLine::Span(_)) => ResolvedAxis::Auto(n),
        (GridLine::Auto, GridLine::Auto) => ResolvedAxis::Auto(1),
    }
}

/// Maps a resolved `justify-self` (falling back to the container's
/// `justify-items` when `Auto`) to the inline-axis cell alignment (D5).
fn resolved_inline_alignment(
    container: JustifyItemsKw,
    item: JustifySelfKw,
) -> GridInlineAlignment {
    match item {
        JustifySelfKw::Auto => match container {
            JustifyItemsKw::Normal | JustifyItemsKw::Stretch => GridInlineAlignment::Stretch,
            JustifyItemsKw::Start => GridInlineAlignment::Start,
            JustifyItemsKw::End => GridInlineAlignment::End,
            JustifyItemsKw::Center => GridInlineAlignment::Center,
        },
        JustifySelfKw::Normal | JustifySelfKw::Stretch => GridInlineAlignment::Stretch,
        JustifySelfKw::Start => GridInlineAlignment::Start,
        JustifySelfKw::End => GridInlineAlignment::End,
        JustifySelfKw::Center => GridInlineAlignment::Center,
    }
}

/// Maps a resolved `align-self` (falling back to the container's `align-items`
/// when `None`/`auto`) to the block-axis cell alignment (D5). `start`/`end` and
/// `flex-start`/`flex-end` coincide for the horizontal-writing-mode grid (the
/// block axis is physical vertical).
fn resolved_block_alignment(
    container: AlignItemsKw,
    item: Option<AlignItemsKw>,
) -> GridBlockAxisAlignment {
    let effective = item.unwrap_or(container);
    match effective {
        AlignItemsKw::Stretch => GridBlockAxisAlignment::Stretch,
        AlignItemsKw::Start | AlignItemsKw::FlexStart => GridBlockAxisAlignment::Start,
        AlignItemsKw::End | AlignItemsKw::FlexEnd => GridBlockAxisAlignment::End,
        AlignItemsKw::Center => GridBlockAxisAlignment::Center,
    }
}

/// The container's resolved placement: the column and row line-range
/// classifications for every item, plus the resolved spans and cell starts
/// written into the item lane / view.
///
/// This is §8.5 (D4): definite items resolve their cells and claim first
/// (pass 1), then items locked to the major axis scan the minor axis for the
/// first free span (pass 2), then remaining autos flow per `auto_flow`
/// (pass 3). Negative lines translate by the container-wide `1 − min(start)`
/// offset (D2) before any claim. Returns the resolved implicit grid size.
#[allow(clippy::too_many_lines)] // the §8.5 placement is one linear three-pass sequence
fn resolve_grid_placement(
    scratch: &mut LayoutScratch,
    order_range: LaneRange,
    col_count: usize,
    explicit_row_count: usize,
    auto_flow: GridAutoFlowKw,
) -> GridPlacement {
    let order_len = usize::try_from(order_range.len).expect("order range len fits usize");

    // Pass A: the container-wide offset from the most-negative definite start
    // (D2; `1 − min(start)`). Zero when no definite start resolves negative.
    let mut min_col = 0i32;
    let mut min_row = 0i32;
    for order_pos in 0..order_len {
        let view = scratch
            .grid_items
            .get(order_entry_dom_index(scratch, order_range, order_pos))
            .view;
        if let ResolvedAxis::Definite(start, _) =
            resolve_axis_range(view.column_start, view.column_end, col_count)
        {
            min_col = min_col.min(start);
        }
        if let ResolvedAxis::Definite(start, _) =
            resolve_axis_range(view.row_start, view.row_end, explicit_row_count)
        {
            min_row = min_row.min(start);
        }
    }
    let col_offset = i64::from(-min_col);
    let row_offset = i64::from(-min_row);

    scratch.cell_map.reset();

    // Pass 1: both axes definite (D4 step 1).
    for order_pos in 0..order_len {
        let dom_index = order_entry_dom_index(scratch, order_range, order_pos);
        let view = scratch.grid_items.get(dom_index).view;
        let col = translate_axis(
            resolve_axis_range(view.column_start, view.column_end, col_count),
            col_offset,
        );
        let row = translate_axis(
            resolve_axis_range(view.row_start, view.row_end, explicit_row_count),
            row_offset,
        );
        if let (ResolvedAxis::Definite(cs, ce), ResolvedAxis::Definite(rs, re)) = (col, row) {
            write_placement(scratch, dom_index, cs, ce, rs, re);
        }
    }

    // Pass 2: locked to the major axis (D4 step 2): definite major axis, auto
    // minor axis — scan the minor axis for the first free span.
    for order_pos in 0..order_len {
        let dom_index = order_entry_dom_index(scratch, order_range, order_pos);
        let view = scratch.grid_items.get(dom_index).view;
        let col = translate_axis(
            resolve_axis_range(view.column_start, view.column_end, col_count),
            col_offset,
        );
        let row = translate_axis(
            resolve_axis_range(view.row_start, view.row_end, explicit_row_count),
            row_offset,
        );
        match (col, row) {
            // Row flow: definite row (major) + auto column (minor) — scan
            // columns at the definite row for the first free span.
            (ResolvedAxis::Auto(col_span), ResolvedAxis::Definite(rs, re))
                if is_row_major(auto_flow) =>
            {
                let row_span = usize::try_from(re - rs).expect("positive row span");
                let col_span_usize = usize::try_from(col_span).expect("span fits usize");
                let col = find_auto_col(
                    &scratch.cell_map,
                    0,
                    usize::try_from(rs).expect("row fits"),
                    row_span,
                    col_span_usize,
                );
                let col_i32 = i32::try_from(col).expect("col fits i32");
                write_placement(
                    scratch,
                    dom_index,
                    col_i32,
                    col_i32 + i32::try_from(col_span).expect("span fits i32"),
                    rs,
                    re,
                );
            }
            // Column flow: definite column (major) + auto row (minor) — scan
            // rows at the definite column for the first free span.
            (ResolvedAxis::Definite(cs, ce), ResolvedAxis::Auto(row_span))
                if !is_row_major(auto_flow) =>
            {
                let col_span = usize::try_from(ce - cs).expect("positive col span");
                let row_span_usize = usize::try_from(row_span).expect("span fits usize");
                let row = find_auto_row(
                    &scratch.cell_map,
                    0,
                    usize::try_from(cs).expect("col fits"),
                    col_span,
                    row_span_usize,
                );
                let row_i32 = i32::try_from(row).expect("row fits i32");
                write_placement(
                    scratch,
                    dom_index,
                    cs,
                    ce,
                    row_i32,
                    row_i32 + i32::try_from(row_span).expect("span fits i32"),
                );
            }
            _ => {}
        }
    }

    // Pass 3: remaining autos per auto_flow (D4 step 3). The wrap bounds are
    // the implicit grid width/height established by the explicit tracks plus
    // the definite/locked items (auto items wrap rather than grow the minor
    // axis), captured BEFORE this pass.
    let grid_width = scratch.cell_map.width().max(col_count.max(1));
    let grid_height = scratch.cell_map.height().max(explicit_row_count.max(1));
    place_remaining_autos(
        scratch,
        order_range,
        col_count,
        explicit_row_count,
        col_offset,
        row_offset,
        auto_flow,
        grid_width,
        grid_height,
    );

    GridPlacement {
        total_col_count: scratch.cell_map.width().max(grid_width),
        total_row_count: scratch.cell_map.height().max(grid_height),
        col_offset: usize::try_from(col_offset).expect("column offset fits usize"),
        row_offset: usize::try_from(row_offset).expect("row offset fits usize"),
    }
}

/// The order lane's `dom_index` at `order_pos` within `order_range`.
fn order_entry_dom_index(scratch: &LayoutScratch, order_range: LaneRange, order_pos: usize) -> u32 {
    scratch.order.slice(order_range)[order_pos].dom_index
}

/// True for row-major auto-flow (`Row`/`RowDense`).
fn is_row_major(auto_flow: GridAutoFlowKw) -> bool {
    matches!(auto_flow, GridAutoFlowKw::Row | GridAutoFlowKw::RowDense)
}

/// True for dense packing (`RowDense`/`ColumnDense`).
fn is_dense(auto_flow: GridAutoFlowKw) -> bool {
    matches!(
        auto_flow,
        GridAutoFlowKw::RowDense | GridAutoFlowKw::ColumnDense
    )
}

/// Translates a resolved axis by the container offset, making every definite
/// line non-negative (auto spans are unchanged).
fn translate_axis(axis: ResolvedAxis, offset: i64) -> ResolvedAxis {
    match axis {
        ResolvedAxis::Definite(start, end) => ResolvedAxis::Definite(
            i32::try_from(i64::from(start) + offset).expect("translated line fits i32"),
            i32::try_from(i64::from(end) + offset).expect("translated line fits i32"),
        ),
        ResolvedAxis::Auto(span) => ResolvedAxis::Auto(span),
    }
}

/// Claims a definite cell and writes the item's resolved cell + spans into the
/// item lane / view.
fn write_placement(
    scratch: &mut LayoutScratch,
    dom_index: u32,
    col_start: i32,
    col_end: i32,
    row_start: i32,
    row_end: i32,
) {
    let col_span = u32::try_from(col_end - col_start).expect("positive column span");
    let row_span = u32::try_from(row_end - row_start).expect("positive row span");
    scratch.cell_map.claim(
        usize::try_from(row_start).expect("row fits usize"),
        usize::try_from(col_start).expect("column fits usize"),
        usize::try_from(row_span).expect("row span fits usize"),
        usize::try_from(col_span).expect("column span fits usize"),
    );
    let entry = scratch.grid_items.get_mut(dom_index);
    entry.col = u32::try_from(col_start).expect("column fits u32");
    entry.row = u32::try_from(row_start).expect("row fits u32");
    entry.view.col_span = col_span;
    entry.view.row_span = row_span;
}

/// Finds the first column `>= start_col` where the `(row_span × col_span)`
/// rectangle at `(row, col)` is free.
fn find_auto_col(
    cell_map: &GridCellMap,
    start_col: usize,
    row: usize,
    row_span: usize,
    col_span: usize,
) -> usize {
    let mut col = start_col;
    while !cell_map.is_free(row, col, row_span, col_span) {
        col += 1;
    }
    col
}

/// Finds the first row `>= start_row` where the `(row_span × col_span)`
/// rectangle at `(row, col)` is free.
fn find_auto_row(
    cell_map: &GridCellMap,
    start_row: usize,
    col: usize,
    col_span: usize,
    row_span: usize,
) -> usize {
    let mut row = start_row;
    while !cell_map.is_free(row, col, row_span, col_span) {
        row += 1;
    }
    row
}

/// Pass 3 (D4 step 3): the remaining auto items, placed per `auto_flow`
/// (row/column × sparse/dense) with a two-axis cursor.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn place_remaining_autos(
    scratch: &mut LayoutScratch,
    order_range: LaneRange,
    col_count: usize,
    explicit_row_count: usize,
    col_offset: i64,
    row_offset: i64,
    auto_flow: GridAutoFlowKw,
    grid_width: usize,
    grid_height: usize,
) {
    let order_len = usize::try_from(order_range.len).expect("order range len fits usize");
    let row_major = is_row_major(auto_flow);
    let dense = is_dense(auto_flow);
    let mut cursor_row = 0usize;
    let mut cursor_col = 0usize;

    for order_pos in 0..order_len {
        let dom_index = order_entry_dom_index(scratch, order_range, order_pos);
        let view = scratch.grid_items.get(dom_index).view;
        let col = translate_axis(
            resolve_axis_range(view.column_start, view.column_end, col_count),
            col_offset,
        );
        let row = translate_axis(
            resolve_axis_range(view.row_start, view.row_end, explicit_row_count),
            row_offset,
        );

        // Remaining items have an auto major axis. `let … else` skips items
        // already placed in pass 1 (both definite) or pass 2 (locked to major).
        if row_major {
            let ResolvedAxis::Auto(row_span) = row else {
                continue;
            };
            match col {
                ResolvedAxis::Definite(cs, ce) => {
                    // Definite column (minor), auto row: wrap the cursor to the
                    // definite column, then scan rows there.
                    let cs_usize = usize::try_from(cs).expect("column fits usize");
                    if !dense && cs_usize < cursor_col {
                        cursor_row += 1;
                    }
                    let col_span = u32::try_from(ce - cs).expect("positive column span");
                    let col_span_usize = usize::try_from(col_span).expect("span fits usize");
                    let row_span_usize = usize::try_from(row_span).expect("span fits usize");
                    let found = find_auto_row(
                        &scratch.cell_map,
                        cursor_row,
                        cs_usize,
                        col_span_usize,
                        row_span_usize,
                    );
                    write_placement(
                        scratch,
                        dom_index,
                        cs,
                        ce,
                        i32::try_from(found).expect("row fits i32"),
                        i32::try_from(found + row_span_usize).expect("row fits i32"),
                    );
                    if dense {
                        cursor_row = 0;
                        cursor_col = 0;
                    } else {
                        cursor_row = found;
                        cursor_col = cs_usize;
                    }
                }
                ResolvedAxis::Auto(col_span) => {
                    let col_span_usize = usize::try_from(col_span).expect("span fits usize");
                    let row_span_usize = usize::try_from(row_span).expect("span fits usize");
                    let (found_row, found_col) = find_auto_auto_row(
                        &scratch.cell_map,
                        cursor_row,
                        cursor_col,
                        row_span_usize,
                        col_span_usize,
                        grid_width,
                    );
                    write_placement(
                        scratch,
                        dom_index,
                        i32::try_from(found_col).expect("column fits i32"),
                        i32::try_from(found_col + col_span_usize).expect("column fits i32"),
                        i32::try_from(found_row).expect("row fits i32"),
                        i32::try_from(found_row + row_span_usize).expect("row fits i32"),
                    );
                    if dense {
                        cursor_row = 0;
                        cursor_col = 0;
                    } else {
                        cursor_row = found_row;
                        cursor_col = found_col + col_span_usize;
                    }
                }
            }
        } else {
            let ResolvedAxis::Auto(col_span) = col else {
                continue;
            };
            match row {
                ResolvedAxis::Definite(rs, re) => {
                    // Definite row (minor), auto column: wrap the cursor to the
                    // definite row, then scan columns there.
                    let rs_usize = usize::try_from(rs).expect("row fits usize");
                    if !dense && rs_usize < cursor_row {
                        cursor_col += 1;
                    }
                    let row_span = u32::try_from(re - rs).expect("positive row span");
                    let row_span_usize = usize::try_from(row_span).expect("span fits usize");
                    let col_span_usize = usize::try_from(col_span).expect("span fits usize");
                    let found = find_auto_col(
                        &scratch.cell_map,
                        cursor_col,
                        rs_usize,
                        row_span_usize,
                        col_span_usize,
                    );
                    write_placement(
                        scratch,
                        dom_index,
                        i32::try_from(found).expect("column fits i32"),
                        i32::try_from(found + col_span_usize).expect("column fits i32"),
                        rs,
                        re,
                    );
                    if dense {
                        cursor_row = 0;
                        cursor_col = 0;
                    } else {
                        cursor_row = rs_usize;
                        cursor_col = found;
                    }
                }
                ResolvedAxis::Auto(row_span) => {
                    let col_span_usize = usize::try_from(col_span).expect("span fits usize");
                    let row_span_usize = usize::try_from(row_span).expect("span fits usize");
                    let (found_row, found_col) = find_auto_auto_col(
                        &scratch.cell_map,
                        cursor_row,
                        cursor_col,
                        row_span_usize,
                        col_span_usize,
                        grid_height,
                    );
                    write_placement(
                        scratch,
                        dom_index,
                        i32::try_from(found_col).expect("column fits i32"),
                        i32::try_from(found_col + col_span_usize).expect("column fits i32"),
                        i32::try_from(found_row).expect("row fits i32"),
                        i32::try_from(found_row + row_span_usize).expect("row fits i32"),
                    );
                    if dense {
                        cursor_row = 0;
                        cursor_col = 0;
                    } else {
                        cursor_row = found_row + row_span_usize;
                        cursor_col = found_col;
                    }
                }
            }
        }
    }
}

/// Finds the first free `(row, col)` in row-major order from `(start_row,
/// start_col)` for a `(row_span × col_span)` rectangle; wraps to the next row
/// when a row cannot fit the span within the current grid width.
fn find_auto_auto_row(
    cell_map: &GridCellMap,
    start_row: usize,
    start_col: usize,
    row_span: usize,
    col_span: usize,
    grid_width: usize,
) -> (usize, usize) {
    let mut row = start_row;
    let mut col = start_col;
    loop {
        col = find_auto_col(cell_map, col, row, row_span, col_span);
        if col + col_span <= grid_width {
            return (row, col);
        }
        row += 1;
        col = 0;
    }
}

/// Finds the first free `(row, col)` in column-major order from `(start_row,
/// start_col)` for a `(row_span × col_span)` rectangle; wraps to the next
/// column when a column cannot fit the span within the current grid height.
fn find_auto_auto_col(
    cell_map: &GridCellMap,
    start_row: usize,
    start_col: usize,
    row_span: usize,
    col_span: usize,
    grid_height: usize,
) -> (usize, usize) {
    let mut row = start_row;
    let mut col = start_col;
    loop {
        row = find_auto_row(cell_map, row, col, col_span, row_span);
        if row + row_span <= grid_height {
            return (row, col);
        }
        col += 1;
        row = 0;
    }
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
        // Anonymous runs are generated items: no authored placement lines.
        column_start: GridLine::Auto,
        column_end: GridLine::Auto,
        row_start: GridLine::Auto,
        row_end: GridLine::Auto,
        justify_self: JustifySelfKw::Auto,
        align_self: None,
        col_span: 1,
        row_span: 1,
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
    let grid_group = node.stored_computed_groups().grid;
    let align_self = node.computed_flex().align_self;
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
        column_start: grid_group.column_start,
        column_end: grid_group.column_end,
        row_start: grid_group.row_start,
        row_end: grid_group.row_end,
        justify_self: grid_group.justify_self,
        align_self,
        col_span: 1,
        row_span: 1,
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
    scratch: &LayoutScratch,
    item_range: LaneRange,
    col_count: usize,
    col_offset: usize,
) -> (
    [f32; GRID_TRACK_LIST_CAPACITY],
    [f32; GRID_TRACK_LIST_CAPACITY],
) {
    let mut col_h_min = [0.0f32; GRID_TRACK_LIST_CAPACITY];
    let mut col_h_max = [0.0f32; GRID_TRACK_LIST_CAPACITY];
    for entry in scratch.grid_items.slice(item_range) {
        let col_start = usize::try_from(entry.col).expect("col fits usize");
        let col_end = col_start
            .saturating_add(usize::try_from(entry.view.col_span).expect("span fits usize"));
        for col in col_start..col_end {
            let Some(explicit) = col.checked_sub(col_offset) else {
                continue; // implicit column (left of the explicit grid)
            };
            if explicit >= col_count {
                continue; // implicit column (right of the explicit grid)
            }
            col_h_min[explicit] = col_h_min[explicit].max(entry.view.h_min_content_px);
            col_h_max[explicit] = col_h_max[explicit].max(entry.view.h_max_content_px);
        }
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

/// A zeroed track entry (an implicit track's placeholder before its content is
/// resolved).
fn empty_track_entry() -> GridTrackEntry {
    GridTrackEntry {
        base_px: 0.0,
        limit_px: 0.0,
        state_bits: 0,
        flex_factor: 0.0,
        is_flexible: false,
    }
}

/// Sizes the column tracks (D2/D3): explicit columns by the phase algorithm
/// (init → maximize → fr expand → stretch auto), implicit columns (left of the
/// offset from negative lines, and right beyond the explicit count) by
/// `auto_columns` content.
#[allow(clippy::too_many_arguments)]
fn size_column_tracks(
    scratch: &mut LayoutScratch,
    column_tracks: &[GridTrackSize; GRID_TRACK_LIST_CAPACITY],
    col_count: usize,
    total_col_count: usize,
    col_offset: usize,
    item_range: LaneRange,
    col_h_min: &[f32; GRID_TRACK_LIST_CAPACITY],
    col_h_max: &[f32; GRID_TRACK_LIST_CAPACITY],
    container_width_px: f32,
    column_gap_px: f32,
    auto_columns: GridTrackSize,
) -> LaneRange {
    let range_start = scratch.tracks.begin_range();

    // Left implicit columns (negative-line offset).
    for _ in 0..col_offset {
        scratch.tracks.push(empty_track_entry());
    }
    // Explicit columns.
    for col in 0..col_count {
        scratch.tracks.push(init_column_track(
            &column_tracks[col],
            col_h_min[col],
            col_h_max[col],
            container_width_px,
        ));
    }
    // Right implicit columns (beyond the explicit count).
    let right_implicit = total_col_count.saturating_sub(col_offset + col_count);
    for _ in 0..right_implicit {
        scratch.tracks.push(empty_track_entry());
    }
    let range = scratch.tracks.finish_range(range_start);

    // Accumulate each implicit column's content (max h_max of its items), then
    // resolve it through `auto_columns`.
    let explicit_start = col_offset;
    let explicit_end = col_offset + col_count;
    for entry in scratch.grid_items.slice(item_range) {
        let col_start = usize::try_from(entry.col).expect("col fits usize");
        let col_end = col_start
            .saturating_add(usize::try_from(entry.view.col_span).expect("span fits usize"));
        for col in col_start..col_end {
            if col >= explicit_start && col < explicit_end {
                continue; // explicit columns are sized by the phase algorithm
            }
            let track = scratch
                .tracks
                .get_mut(range.start + u32::try_from(col).expect("col fits u32"));
            track.base_px = track.base_px.max(entry.view.h_max_content_px);
        }
    }
    let mut implicit_total = 0.0f32;
    for col in 0..total_col_count {
        if col >= explicit_start && col < explicit_end {
            continue;
        }
        let track = scratch
            .tracks
            .get_mut(range.start + u32::try_from(col).expect("col fits u32"));
        let content = track.base_px;
        let width = resolve_implicit_col_width(auto_columns, content, container_width_px);
        track.base_px = width;
        track.limit_px = width;
        implicit_total += width;
    }

    // Phase algorithm on the explicit sub-range, against the width left after
    // the implicit columns consume their share.
    let explicit_range = LaneRange {
        start: range.start + u32::try_from(col_offset).expect("col offset fits u32"),
        len: u32::try_from(col_count).expect("col count fits u32"),
    };
    run_track_sizing_phases(
        scratch,
        explicit_range,
        (container_width_px - implicit_total).max(0.0),
        column_gap_px,
    );
    range
}

/// Resolves one implicit column's width from `auto_columns` (the `auto_rows`
/// leaf, mirrored against the container's width axis).
fn resolve_implicit_col_width(
    auto_columns: GridTrackSize,
    content_width_px: f32,
    container_width_px: f32,
) -> f32 {
    let min = match auto_columns.min {
        TrackBreadth::Auto => 0.0,
        TrackBreadth::Px(px) => px,
        TrackBreadth::Percent(p) => container_width_px * p * 0.01,
        TrackBreadth::MinContent | TrackBreadth::MaxContent => content_width_px,
    };
    let max = match auto_columns.max {
        TrackMaxBreadth::Auto | TrackMaxBreadth::Fr(_) => f32::INFINITY,
        TrackMaxBreadth::Px(px) => px,
        TrackMaxBreadth::Percent(p) => container_width_px * p * 0.01,
        TrackMaxBreadth::MinContent | TrackMaxBreadth::MaxContent => content_width_px,
    };
    // CSS Grid §7.2.1: min > max → the max is treated as min.
    let max = if min > max { min } else { max };
    content_width_px.clamp(min, max)
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
    // cap applies per iteration to the CURRENT remaining factor sum: §7.2.4's
    // restart recomputes the used flex fraction over the remaining tracks, so a
    // freeze under Σfr < 1 re-applies the cap to the reduced factor sum. The
    // loop terminates because each pass freezes ≥1 track or breaks.
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
            // redistributable pool. The space stays UNSCALED: the sum<1 cap is
            // applied per iteration to the CURRENT remaining factor sum.
            let flex_base_sum: f32 = entries
                .iter()
                .filter(|entry| entry.flex_factor > 0.0)
                .map(|entry| entry.base_px)
                .sum();
            let mut free = remaining + flex_base_sum;
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
                let fraction = free * factor_sum.min(1.0) / factor_sum;
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

/// Resolves each item's content width (stretch/definite/justify-exempt) and row
/// cross contribution (refold for flagged-and-width-changed items), writing both
/// into the item lane. The resolved cell and spans were already written by
/// §8.5 placement (D4); this pass consumes them.
#[allow(clippy::too_many_arguments)]
fn resolve_grid_items<R>(
    root_dom: &mut RootDom,
    tree_size: (f32, f32),
    child_context: &LayoutContext,
    order_range: LaneRange,
    column_range: LaneRange,
    column_gap_px: f32,
    grid_group: &ComputedGridGroup,
    resolver: &mut R,
    metric: RootDomMetricName,
) -> Result<(), DomError>
where
    R: DomLayoutResolver + ?Sized,
{
    let order_len = usize::try_from(order_range.len).expect("order range len fits usize");
    for order_pos in 0..order_len {
        let (dom_index, view, horizontal_bp, vertical_bp, track_width_px) = {
            let order_entries = root_dom.layout_scratch.order.slice(order_range);
            let order_entry = order_entries[order_pos];
            let entry = root_dom
                .layout_scratch
                .grid_items
                .get(order_entry.dom_index);
            let track_width = track_span_width(
                &root_dom.layout_scratch,
                column_range,
                entry.col,
                entry.view.col_span,
                column_gap_px,
            );
            (
                order_entry.dom_index,
                entry.view,
                entry.horizontal_border_padding_px,
                entry.vertical_border_padding_px,
                track_width,
            )
        };
        // The resolved content width: definite width, auto horizontal margins
        // (fit-content), justify stretch (track width minus insets), else the
        // natural content width for start/end/center (D5).
        let (child_style, _) = resolved_node_style(root_dom, view.source.key(), child_context)?;
        let inline_alignment =
            resolved_inline_alignment(grid_group.justify_items, view.justify_self);
        let resolved_content_width = resolved_item_content_width(
            &child_style,
            &view,
            horizontal_bp,
            track_width_px,
            inline_alignment,
        );
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
        entry.resolved_content_width_px = resolved_content_width;
        entry.resolved_cross_contribution_px = cross_contribution;
    }
    Ok(())
}

/// The sum of the track sizes spanned by an item (`start..start+span`) plus the
/// gaps between them (D4: a span-N item's track width includes N tracks and
/// N−1 gaps).
fn track_span_width(
    scratch: &LayoutScratch,
    range: LaneRange,
    start: u32,
    span: u32,
    gap_px: f32,
) -> f32 {
    let mut width = 0.0f32;
    for offset in 0..span {
        width += scratch.tracks.get(range.start + start + offset).base_px;
    }
    width + gap_px * usize_to_f32(usize::try_from(span.saturating_sub(1)).expect("span fits usize"))
}

/// True when the item has an auto left or right margin (fit-content inline, not
/// stretched).
fn horizontal_auto_margins(view: &GridItemView) -> bool {
    view.auto_margins & (MARGIN_LEFT_AUTO | MARGIN_RIGHT_AUTO) != 0
}

/// The item's resolved content width (D5): definite → definite; auto horizontal
/// margins → natural content (fit-content); stretch → track width minus insets;
/// start/end/center → natural content (not stretched).
fn resolved_item_content_width(
    child_style: &NodeStyle,
    view: &GridItemView,
    horizontal_bp: f32,
    track_width_px: f32,
    inline_alignment: GridInlineAlignment,
) -> f32 {
    match child_style.width_px {
        Some(definite) => definite,
        None if horizontal_auto_margins(view) => view.content_main_px,
        None => match inline_alignment {
            GridInlineAlignment::Stretch => (track_width_px - horizontal_bp).max(0.0),
            GridInlineAlignment::Start | GridInlineAlignment::End | GridInlineAlignment::Center => {
                view.content_main_px
            }
        },
    }
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

/// Sizes the row tracks (D3): explicit rows by the SAME phase algorithm as
/// columns, against the container's definite height; when the height is
/// indefinite, explicit rows size to content (the T1 implicit-row rule).
/// Implicit rows (left of the offset and right beyond the explicit count) size
/// by `auto_rows` content.
#[allow(clippy::too_many_arguments)]
fn size_row_tracks(
    scratch: &mut LayoutScratch,
    item_range: LaneRange,
    row_tracks: &[GridTrackSize; GRID_TRACK_LIST_CAPACITY],
    explicit_row_count: usize,
    total_row_count: usize,
    row_offset: usize,
    auto_rows: GridTrackSize,
    container_height_px: Option<f32>,
) -> LaneRange {
    let range_start = scratch.tracks.begin_range();

    // Per-explicit-row max cross contribution (items spanning the row).
    let mut row_cross = [0.0f32; GRID_TRACK_LIST_CAPACITY];
    for entry in scratch.grid_items.slice(item_range) {
        let row_start = usize::try_from(entry.row).expect("row fits usize");
        let row_end = row_start
            .saturating_add(usize::try_from(entry.view.row_span).expect("span fits usize"));
        for row in row_start..row_end {
            let Some(explicit) = row.checked_sub(row_offset) else {
                continue; // implicit row left of the explicit grid
            };
            if explicit >= explicit_row_count {
                continue; // implicit row right of the explicit grid
            }
            row_cross[explicit] = row_cross[explicit].max(entry.resolved_cross_contribution_px);
        }
    }

    // Left implicit rows.
    for _ in 0..row_offset {
        scratch.tracks.push(empty_track_entry());
    }
    // Explicit rows: phase-initialized against a definite height, or resolved to
    // content when the height is indefinite.
    for row in 0..explicit_row_count {
        let track = &row_tracks[row];
        let cross = row_cross[row];
        let entry = if let Some(height) = container_height_px {
            init_column_track(track, cross, cross, height)
        } else {
            let content_height = resolve_implicit_row_height(*track, cross, None);
            GridTrackEntry {
                base_px: content_height,
                limit_px: content_height,
                state_bits: 0,
                flex_factor: 0.0,
                is_flexible: false,
            }
        };
        scratch.tracks.push(entry);
    }
    // Right implicit rows.
    let right_implicit = total_row_count.saturating_sub(row_offset + explicit_row_count);
    for _ in 0..right_implicit {
        scratch.tracks.push(empty_track_entry());
    }
    let range = scratch.tracks.finish_range(range_start);

    // Accumulate each implicit row's content (max cross of its items), then
    // resolve it through `auto_rows`.
    let explicit_start = row_offset;
    let explicit_end = row_offset + explicit_row_count;
    for entry in scratch.grid_items.slice(item_range) {
        let row_start = usize::try_from(entry.row).expect("row fits usize");
        let row_end = row_start
            .saturating_add(usize::try_from(entry.view.row_span).expect("span fits usize"));
        for row in row_start..row_end {
            if row >= explicit_start && row < explicit_end {
                continue;
            }
            let track = scratch
                .tracks
                .get_mut(range.start + u32::try_from(row).expect("row fits u32"));
            track.base_px = track.base_px.max(entry.resolved_cross_contribution_px);
        }
    }
    let mut implicit_total = 0.0f32;
    for row in 0..total_row_count {
        if row >= explicit_start && row < explicit_end {
            continue;
        }
        let track = scratch
            .tracks
            .get_mut(range.start + u32::try_from(row).expect("row fits u32"));
        let content = track.base_px;
        let height = resolve_implicit_row_height(auto_rows, content, container_height_px);
        track.base_px = height;
        track.limit_px = height;
        implicit_total += height;
    }

    // Phase algorithm on the explicit sub-range (definite height only).
    if let Some(height) = container_height_px {
        let explicit_range = LaneRange {
            start: range.start + u32::try_from(row_offset).expect("row offset fits u32"),
            len: u32::try_from(explicit_row_count).expect("row count fits u32"),
        };
        run_track_sizing_phases(
            scratch,
            explicit_range,
            (height - implicit_total).max(0.0),
            0.0,
        );
    }
    range
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
    // CSS Grid §7.2.1: when `minmax(min, max)` has min > max, the max is
    // treated as min (the min wins).
    let max = if min > max { min } else { max };
    content_cross_px.clamp(min, max)
}

/// The container's resolved content height: Σ row heights + row gaps.
fn total_row_height(scratch: &LayoutScratch, row_range: LaneRange, row_gap_px: f32) -> f32 {
    let rows = scratch.tracks.slice(row_range);
    let sum: f32 = rows.iter().map(|entry| entry.base_px).sum();
    let gap_total = row_gap_px * usize_to_f32(rows.len().saturating_sub(1));
    (sum + gap_total).max(0.0)
}

/// Places each item into its cell (D5): resolve the inline-axis (justify) and
/// block-axis (align) alignment, stretch/definite/ratio content overrides,
/// absorbed auto margins, and relayout the child subtree at the resolved cell
/// rect (across the item's spans). Returns the container content height.
#[allow(clippy::too_many_arguments)]
fn place_grid_children<R>(
    root_dom: &mut RootDom,
    tree_size: (f32, f32),
    child_context: &LayoutContext,
    solve: GridContainerSolve,
    column_gap_px: f32,
    row_gap_px: f32,
    grid_group: &ComputedGridGroup,
    container_align_items: AlignItemsKw,
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
                track_span_width(
                    &root_dom.layout_scratch,
                    solve.column_range,
                    entry.col,
                    entry.view.col_span,
                    column_gap_px,
                ),
                track_span_width(
                    &root_dom.layout_scratch,
                    solve.row_range,
                    entry.row,
                    entry.view.row_span,
                    row_gap_px,
                ),
            )
        };
        let entry = *root_dom.layout_scratch.grid_items.get(dom_index);
        let (child_style, _) =
            resolved_node_style(root_dom, entry.view.source.key(), child_context)?;
        let inline_alignment =
            resolved_inline_alignment(grid_group.justify_items, entry.view.justify_self);
        let block_alignment =
            resolved_block_alignment(container_align_items, entry.view.align_self);
        let placement = resolve_grid_child_placement(
            child_context,
            cell_left,
            cell_top,
            track_width,
            row_height,
            &child_style,
            &entry,
            inline_alignment,
            block_alignment,
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
/// justify/align offsets + absorbed auto start margins), the
/// stretch/definite/ratio content overrides, and the four absorbed auto
/// margins. Absorbed auto margins take precedence over justify/align (D5).
#[allow(clippy::too_many_arguments)]
fn resolve_grid_child_placement(
    child_context: &LayoutContext,
    cell_left_px: f32,
    cell_top_px: f32,
    track_width_px: f32,
    row_height_px: f32,
    child_style: &NodeStyle,
    entry: &GridItemLaneEntry,
    inline_alignment: GridInlineAlignment,
    block_alignment: GridBlockAxisAlignment,
) -> GridChildPlacement {
    let view = &entry.view;
    let horizontal_bp = entry.horizontal_border_padding_px;
    let vertical_bp = entry.vertical_border_padding_px;
    let horizontal_auto = view.auto_margins & (MARGIN_LEFT_AUTO | MARGIN_RIGHT_AUTO) != 0;
    let vertical_auto = view.auto_margins & (MARGIN_TOP_AUTO | MARGIN_BOTTOM_AUTO) != 0;

    // Resolved content width: definite → definite; auto horizontal margins →
    // fit-content (natural, not stretched); stretch → track width; start/end/
    // center → natural content width.
    let resolved_content_width = resolved_item_content_width(
        child_style,
        view,
        horizontal_bp,
        track_width_px,
        inline_alignment,
    );

    // Content-height override: definite → None (style owns it); replaced-ratio
    // → None (ratio-derived from width); auto vertical margins → None (natural);
    // block stretch → row height; else (start/end/center) → None (natural).
    let content_height_override =
        if child_style.height_px.is_some() || view.replaced_ratio || vertical_auto {
            None
        } else {
            match block_alignment {
                GridBlockAxisAlignment::Stretch => Some((row_height_px - vertical_bp).max(0.0)),
                GridBlockAxisAlignment::Start
                | GridBlockAxisAlignment::End
                | GridBlockAxisAlignment::Center => None,
            }
        };

    // Absorbed auto margins (CSS Box Alignment §8.1): free = cell − border-box
    // − non-auto margins, distributed equally among that axis's auto margins.
    let border_box_width = resolved_content_width + horizontal_bp;
    let free_width =
        (track_width_px - border_box_width - view.margins[1] - view.margins[3]).max(0.0);
    let (absorbed_left_px, absorbed_right_px) = absorbed_auto_margins(
        view,
        horizontal_auto,
        free_width,
        MARGIN_LEFT_AUTO,
        MARGIN_RIGHT_AUTO,
    );
    let resolved_content_height = child_style.height_px.unwrap_or(view.content_cross_px);
    let border_box_height = resolved_content_height + vertical_bp;
    let free_height =
        (row_height_px - border_box_height - view.margins[0] - view.margins[2]).max(0.0);
    let (absorbed_top_px, absorbed_bottom_px) = absorbed_auto_margins(
        view,
        vertical_auto,
        free_height,
        MARGIN_TOP_AUTO,
        MARGIN_BOTTOM_AUTO,
    );

    // Justify/align offsets (D5); absorbed auto margins take precedence, so an
    // axis with absorbed auto margins has a zero alignment offset.
    let justify_offset = if horizontal_auto {
        0.0
    } else {
        match inline_alignment {
            GridInlineAlignment::Stretch | GridInlineAlignment::Start => 0.0,
            GridInlineAlignment::End => free_width,
            GridInlineAlignment::Center => free_width / 2.0,
        }
    };
    let align_offset = if vertical_auto {
        0.0
    } else {
        match block_alignment {
            GridBlockAxisAlignment::Stretch | GridBlockAxisAlignment::Start => 0.0,
            GridBlockAxisAlignment::End => free_height,
            GridBlockAxisAlignment::Center => free_height / 2.0,
        }
    };

    let context = LayoutContext {
        flow_left_px: child_context.flow_left_px + cell_left_px + justify_offset + absorbed_left_px,
        flow_top_px: child_context.flow_top_px + cell_top_px + align_offset + absorbed_top_px,
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

/// Lays out each EXCLUDED abs-pos/fixed child at its static position (D8): a
/// both-axes-definite child gets the resolved grid-area rectangle (CSS Grid
/// §8.4); any other child falls back to the container's padding-box top-left
/// (auto lines fall back to padding-box edges), positioned per CSS 2.1
/// §10.3.7/§10.6.4 against the padding box.
#[allow(clippy::too_many_arguments)]
fn place_excluded_abs_grid_children<R>(
    root_dom: &mut RootDom,
    tree_size: (f32, f32),
    container_key: DomNodeSlotKey,
    entered: &EnteredNodeLayout,
    child_context: &LayoutContext,
    content_width_px: f32,
    content_height_px: f32,
    column_range: LaneRange,
    row_range: LaneRange,
    col_offset: usize,
    row_offset: usize,
    column_gap_px: f32,
    row_gap_px: f32,
    col_count: usize,
    explicit_row_count: usize,
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
            // The static position: the resolved grid-area rectangle for a
            // both-axes-definite child (D8), else the padding-box origin (auto
            // lines fall back to padding-box edges); the content origin adds the
            // padding.
            let content_left_px = padding_left_px + entered.style.sides.padding.left_px;
            let content_top_px = padding_top_px + entered.style.sides.padding.top_px;
            let (area_left, area_top) = resolve_abs_grid_area(
                root_dom,
                current_child_key,
                col_count,
                explicit_row_count,
                column_range,
                row_range,
                col_offset,
                row_offset,
                column_gap_px,
                row_gap_px,
            );
            let (static_left, static_top) = match (area_left, area_top) {
                (Some(left), Some(top)) => (content_left_px + left, content_top_px + top),
                _ => (content_left_px, content_top_px),
            };
            write_node_border_box_position(root_dom, current_child_key, static_left, static_top)?;
        }
        child_key = next_child_key;
    }
    Ok(())
}

/// Resolves an abs-pos child's grid-area static-position offset when BOTH axes
/// are definite (CSS Grid §8.4), returning `(left, top)` in content-box
/// coordinates; `None` when either axis is auto (padding-box fallback).
#[allow(clippy::too_many_arguments)]
fn resolve_abs_grid_area(
    root_dom: &RootDom,
    child_key: DomNodeSlotKey,
    col_count: usize,
    explicit_row_count: usize,
    column_range: LaneRange,
    row_range: LaneRange,
    col_offset: usize,
    row_offset: usize,
    column_gap_px: f32,
    row_gap_px: f32,
) -> (Option<f32>, Option<f32>) {
    let Some(node) = root_dom.nodes.get(child_key) else {
        return (None, None);
    };
    let grid = node.stored_computed_groups().grid;
    let col = resolve_axis_range(grid.column_start, grid.column_end, col_count);
    let row = resolve_axis_range(grid.row_start, grid.row_end, explicit_row_count);
    let (ResolvedAxis::Definite(col_start, _), ResolvedAxis::Definite(row_start, _)) = (col, row)
    else {
        return (None, None);
    };
    let col_index = usize::try_from(i64::from(col_start) + i64::try_from(col_offset).unwrap_or(0));
    let row_index = usize::try_from(i64::from(row_start) + i64::try_from(row_offset).unwrap_or(0));
    let (Ok(col_index), Ok(row_index)) = (col_index, row_index) else {
        return (None, None);
    };
    let left = track_offset_range(
        &root_dom.layout_scratch,
        column_range,
        col_index,
        column_gap_px,
    );
    let top = track_offset_range(&root_dom.layout_scratch, row_range, row_index, row_gap_px);
    (Some(left), Some(top))
}

/// The sum of the first `index` track sizes plus `index` gaps (an axis offset),
/// by `usize` index.
fn track_offset_range(scratch: &LayoutScratch, range: LaneRange, index: usize, gap_px: f32) -> f32 {
    let tracks = scratch.tracks.slice(range);
    let mut offset = 0.0f32;
    for track in tracks.iter().take(index) {
        offset += track.base_px;
    }
    offset + gap_px * usize_to_f32(index)
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
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
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
        let flex = container_node.computed_flex();
        (
            container_node.stored_computed_groups().grid,
            flex.gap[1],
            flex.gap[0],
        )
    };
    let mut column_tracks = empty_track_array();
    let col_count = resolve_columns(root_dom, grid_group.template_columns, &mut column_tracks);
    let mut row_tracks = empty_track_array();
    let explicit_row_count = resolve_rows(root_dom, grid_group.template_rows, &mut row_tracks);

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

    let (item_range, order_range) =
        build_grid_item_formation(root_dom, tree_size, key, &child_context, resolver, metric)?;

    let placement = resolve_grid_placement(
        &mut root_dom.layout_scratch,
        order_range,
        col_count,
        explicit_row_count,
        grid_group.auto_flow,
    );

    let (col_h_min, col_h_max) = column_contributions(
        &root_dom.layout_scratch,
        item_range,
        col_count,
        placement.col_offset,
    );

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
        placement.total_col_count,
        placement.col_offset,
        item_range,
        &col_h_min,
        &col_h_max,
        container_content_width,
        column_gap_px,
        grid_group.auto_columns,
    );

    resolve_grid_items(
        root_dom,
        tree_size,
        &child_context,
        order_range,
        column_range,
        column_gap_px,
        &grid_group,
        resolver,
        metric,
    )?;

    let row_range = size_row_tracks(
        &mut root_dom.layout_scratch,
        item_range,
        &row_tracks,
        explicit_row_count,
        placement.total_row_count,
        placement.row_offset,
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
            tracks.len(),
            0,
            LaneRange::default(),
            &min,
            &max,
            container_width,
            gap,
            GridTrackSize {
                min: TrackBreadth::Auto,
                max: TrackMaxBreadth::Auto,
            },
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
