#![allow(rustdoc::bare_urls)]
// The style pass resolves px/em lengths to exact `f32` literals and the
// computed groups' initial values are bit-exact, so the typed computed-group
// comparisons in this module are deliberate exact checks.
#![allow(clippy::float_cmp)]

//! Layout-impact and text/effects property-table absence pins (PLAN.md B6, B11).
//!
//! Purpose: pin the observable behavior of the layout-impact properties that
//! are NOT in the 69-property table (`presentation_property.rs`). Each name (or
//! value) is dropped + counted, inert, and crash-free. Every row is two-sided:
//! the declaration drop is counted (`assert_decl_drop`) AND the defined
//! no-effect holds for its family — sibling paint order stays DOM order
//! (`z-index`), sticky behaves as static, unadmitted display values fall back
//! to the initial display, `float`/`clear` leave block flow stacking unchanged,
//! and replaced-element sizing stays on the intrinsic path. B11's text/effects
//! tail family (`text-decoration`
//! shorthand + longhands, `box-shadow`, `text-transform`, `text-indent`,
//! `text-shadow`, `text-overflow`, `letter-spacing`, `word-spacing`,
//! `line-clamp`/`-webkit-line-clamp`, `hyphens`, `vertical-align`,
//! `writing-mode`, `unicode-bidi`, `visibility`, `cursor`, `user-select`,
//! `list-style*`, filters, blend modes, masks, `image-rendering`,
//! `border-style`, table-*, column-*, `orphans`/`widows`, `animation*`, the
//! value-function family, and the `content`/counter properties) lives here as a
//! second table family after the `// — B11 … — //` banner.
//!
//! Class note: every row is `unsupported_*` = unimplemented — policy-gated
//! absence per the D6 admission policy in `MIGRATION.md` (a named consumer
//! need + spec citation + Blink/Gecko algorithm reference + proof). The absent
//! names drop at the name level (`decl_parse.rs`'s `property_shape`); the
//! `position: sticky` and `display: grid`/… rows drop at the VALUE level inside
//! the admitted `position`/`display` grammar.
//!
//! Single-owner note: `overflow-x`/`overflow-y` (B10) are pinned in
//! `unsupported_shorthands.rs` and are NOT repeated here; `text-decoration`
//! (B11) is owned by this file.
//!
//! Honesty notes (divergences from PLAN.md B6's stated model, pinned actual):
//! - **A bare unloaded `img` resolves to 160×0, not 160×90.** The 160×90 is
//!   the intrinsic `ImgIntrinsic` placeholder; its 90px height materializes
//!   only through the intrinsic-ratio transfer when one dimension is authored
//!   (`layout/replaced_elements.rs`). The pin asserts the 160×90 intrinsic and
//!   the ratio-transfer baseline (width 320 → height 180) directly.
//! - **`box-sizing` is on-spec since PR 1 item 3.** It left this file's
//!   unsupported registry when the property was admitted; the relabeled
//!   `box_sizing_border_box_subtracts_insets` proof pins the CSS UI 4 §3.1
//!   relationship (border-box specified size = border box; content box =
//!   specified − padding − border).

use bevy::prelude::World;
use ui_dom::{
    BoxLength, DisplayKw, DomIntrinsic, DomNode, DomNodeSlotKey, DomPointerUpdate, ImgIntrinsic,
    PositionKw, RootDom, StylesheetLoadReport,
};

use crate::layout_proofs::support::{
    assert_append_div_to, assert_append_img_to, assert_append_text_to, assert_close,
    assert_decl_drop, assert_load_root_stylesheet, assert_node_layout, assert_rect_op_fill,
    assert_rect_op_geometry, assert_root_dom_update, assert_set_attr_str, assert_style_set,
    attached_gpu_root_dom_default_viewport_setup, take_staged_layer_paint_plans,
    theme_stylesheet_origin,
};

/// Returns `(world, root, target, report)`: a fresh root with a single
/// class-"x" div target under the document root, `css` loaded (not yet
/// resolved — the caller runs the update), and the stylesheet load report.
fn styled_target_load(css: &str) -> (World, RootDom, DomNodeSlotKey, StylesheetLoadReport) {
    let (world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let target = assert_append_div_to(&mut root, document_root);
    assert_set_attr_str(&mut root, target, "class", "x");
    let report = root
        .context()
        .load_stylesheet(theme_stylesheet_origin(), css)
        .expect("stylesheet should load");
    (world, root, target, report)
}

/// Clones one img node's retained placeholder intrinsic.
fn img_intrinsic(root: &RootDom, key: DomNodeSlotKey) -> ImgIntrinsic {
    match root.node(key).map(DomNode::intrinsic) {
        Some(DomIntrinsic::Img(intrinsic)) => intrinsic.clone(),
        _ => panic!("proof img node should keep img intrinsic state"),
    }
}

// — B6: layout-impact property-table absences — //

#[test]
// UNSUPPORTED (class: unimplemented — D6 admission-gated):
// - SPEC: CSS 2.1 Appendix E (stacking contexts) and CSS Position 3 §3 —
//   `z-index` sets the paint order among positioned siblings within a stacking
//   context. <https://www.w3.org/TR/CSS21/visuren.html#z-index>
// - CURRENT (asserted): both `z-index` declarations drop + count (applied == 2
//   for the two `background-color` declarations, dropped == 2 for the two
//   `z-index` declarations) and the staged paint plan keeps DOM order — the
//   `.a` rect is staged before the `.b` rect, so `z-index` has no effect on
//   sibling paint order.
// - SPEC-CORRECT: `z-index` becomes an order key on the layer
//   (`composite/mod.rs` already maintains `layer_order`); admission per D6.
fn unsupported_z_index_drops_and_dom_order_paints() {
    const SHEET: &str = ".a { z-index: 10; background-color: #ff0000 } \
                         .b { z-index: -1; background-color: #0000ff }";
    assert_decl_drop(SHEET, 2, 2);

    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let a = assert_append_div_to(&mut root, document_root);
    assert_set_attr_str(&mut root, a, "class", "a");
    let b = assert_append_div_to(&mut root, document_root);
    assert_set_attr_str(&mut root, b, "class", "b");
    // Fixed sizes; the second sibling's negative top margin pulls it over the
    // first so the two boxes overlap (the z-index scenario that would reorder
    // paint if the property were honored).
    assert_style_set(&mut root, a, "width", "40px");
    assert_style_set(&mut root, a, "height", "40px");
    assert_style_set(&mut root, b, "width", "40px");
    assert_style_set(&mut root, b, "height", "40px");
    assert_style_set(&mut root, b, "margin-top", "-20px");
    assert_load_root_stylesheet(&root, theme_stylesheet_origin(), SHEET);

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    assert_eq!(plans.len(), 1, "one document-root layer plan");
    assert_eq!(plans[0].rects.len(), 2, "two sibling rects");
    // DOM order: `.a` (red) is staged before `.b` (blue); z-index is dropped,
    // so it cannot reorder sibling paint.
    assert_rect_op_fill(
        &plans[0].rects[0],
        [1.0, 0.0, 0.0, 1.0],
        "first sibling (.a) red",
    );
    assert_rect_op_fill(
        &plans[0].rects[1],
        [0.0, 0.0, 1.0, 1.0],
        "second sibling (.b) blue",
    );
}

#[test]
// UNSUPPORTED (class: unimplemented — D6 admission-gated):
// - SPEC: CSS Position 3 §sticky — `position: sticky` constrains a box to its
//   scroll-port ancestor's constraint rectangle at scroll time.
//   <https://drafts.csswg.org/css-position-3/#sticky-pos>
// - CURRENT (asserted): `position: sticky` drops + counts; the computed
//   `position` stays `PositionKw::Static`, and the target's layout equals the
//   no-declaration baseline — it stacks in normal block flow directly below an
//   identical unstyled sibling.
// - SPEC-CORRECT: `PositionKw::Sticky` plus the sticky constraint algorithm
//   (the scroll machinery at `domains/visitor/scroll.rs`); admission per D6.
fn unsupported_position_sticky_drops_to_static() {
    assert_decl_drop(".x { position: sticky }", 0, 1);

    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let baseline = assert_append_div_to(&mut root, document_root);
    let target = assert_append_div_to(&mut root, document_root);
    assert_set_attr_str(&mut root, target, "class", "x");
    // Identical fixed sizes; only the target carries the sticky declaration.
    assert_style_set(&mut root, baseline, "width", "80px");
    assert_style_set(&mut root, baseline, "height", "12px");
    assert_style_set(&mut root, target, "width", "80px");
    assert_style_set(&mut root, target, "height", "12px");
    assert_load_root_stylesheet(&root, theme_stylesheet_origin(), ".x { position: sticky }");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_eq!(
        root.computed_style(target)
            .expect("target")
            .as_box()
            .position,
        PositionKw::Static,
        "position: sticky should drop to the Static initial",
    );

    let plans = take_staged_layer_paint_plans(&root);
    assert_eq!(plans[0].rects.len(), 2, "baseline + target rects");
    // Normal flow: the sticky target occupies the static block slot directly
    // below the identical unstyled sibling — its block position equals the
    // no-declaration baseline.
    assert_rect_op_geometry(
        &plans[0].rects[0],
        0.0,
        0.0,
        80.0,
        12.0,
        "unstyled baseline sibling",
    );
    assert_rect_op_geometry(
        &plans[0].rects[1],
        0.0,
        12.0,
        80.0,
        12.0,
        "sticky target below baseline",
    );
}

#[test]
// UNSUPPORTED (class: unimplemented — D6 admission-gated):
// - SPEC: CSS Display 3 §2 — `display` admits `grid`, `inline-grid`, `table`,
//   `inline-table`, `flow-root`, `contents`, `ruby` (each with its own box
//   generation / formatting context). <https://drafts.csswg.org/css-display-3/>
// - CURRENT (asserted): each value drops + counts; the computed `display` stays
//   `DisplayKw::Block` for a div (the initial), and the layout equals the
//   block baseline (a single block rect at the authored size).
// - SPEC-CORRECT: `grid`/`table` are new solvers (never-admit by default);
//   `contents` is a box-generation rule — the only display admission with a
//   plausible consumer need; admission per D6.
fn unsupported_display_values_drop_to_initial() {
    struct Case {
        name: &'static str,
        body: &'static str,
    }

    let cases = [
        Case {
            name: "table",
            body: "display: table",
        },
        Case {
            name: "inline_table",
            body: "display: inline-table",
        },
        Case {
            name: "flow_root",
            body: "display: flow-root",
        },
        Case {
            name: "contents",
            body: "display: contents",
        },
        Case {
            name: "ruby",
            body: "display: ruby",
        },
    ];

    for case in &cases {
        assert_decl_drop(&format!(".{} {{ {} }}", case.name, case.body), 0, 1);
    }

    // Representative: `display: table` on a div leaves `DisplayKw::Block` and
    // block layout (the no-declaration baseline). `display: grid`/`inline-grid`
    // are now admitted (see `grid_admission.rs`) — the grid/inline-grid cases
    // left this pin's drop table in the grid T0 landing.
    let (mut world, mut root, target, _report) = styled_target_load(".x { display: table }");
    assert_style_set(&mut root, target, "width", "80px");
    assert_style_set(&mut root, target, "height", "40px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_eq!(
        root.computed_style(target)
            .expect("target")
            .as_box()
            .display,
        DisplayKw::Block,
        "display: table should drop to the Block initial for a div",
    );

    let plans = take_staged_layer_paint_plans(&root);
    assert_eq!(plans.len(), 1, "one document-root layer plan");
    assert_eq!(plans[0].rects.len(), 1, "one block rect");
    assert_rect_op_geometry(
        &plans[0].rects[0],
        0.0,
        0.0,
        80.0,
        40.0,
        "table-dropped block div",
    );
}

#[test]
// ON-SPEC (relabeled from `unsupported_box_sizing_drops_and_border_box_sizing_holds`
// in PR 1 item 3 — the first post-v0.6.0 vocabulary admission):
// - SPEC: CSS UI 4 §3.1 `box-sizing` — `content-box` (the initial) makes
//   `width`/`height` the CONTENT box with padding/border outside it;
//   `border-box` makes `width`/`height` the BORDER box, with the content box
//   computed as `specified − (padding + border)`, floored at 0 per axis.
//   <https://drafts.csswg.org/css-ui-4/#box-sizing>
// - ASSERTED: `box-sizing: border-box; width: 100px` with 10px left + 10px
//   right padding produces a 100px border box (content box 80px); the
//   content-box initial and an explicit `content-box` both produce the
//   unchanged 120px border box. The closed grammar still drops the garbage
//   value `box-sizing: border`.
fn box_sizing_border_box_subtracts_insets() {
    // Drop guard: `border` is not in the closed `content-box | border-box`
    // keyword grammar, so it drops + counts (the grammar stays closed).
    assert_decl_drop(".x { box-sizing: border }", 0, 1);

    // `border-box`: the specified `width: 100px` IS the border box; the 10px
    // left + 10px right padding are laid out inside it, so the border box is
    // exactly 100px and the content box is 80px.
    assert_eq!(
        sized_target_border_box_width(
            ".x { box-sizing: border-box; width: 100px; padding-left: 10px; padding-right: 10px }",
        ),
        100.0,
        "border-box: specified size is the border box",
    );

    // Control A (no box-sizing declared): the content-box initial — the
    // specified 100px is the content box, and the paddings expand the border
    // box to 120px (today's behavior, unchanged).
    assert_eq!(
        sized_target_border_box_width(
            ".x { width: 100px; padding-left: 10px; padding-right: 10px }",
        ),
        120.0,
        "absent box-sizing is the content-box initial",
    );

    // Control B (`content-box` explicit): also 120px — naming the already-active
    // model changes nothing.
    assert_eq!(
        sized_target_border_box_width(
            ".x { box-sizing: content-box; width: 100px; padding-left: 10px; padding-right: 10px }",
        ),
        120.0,
        "explicit content-box matches the initial",
    );
}

#[test]
// OFF-SPEC (divergent — registered in the layout_proofs off-spec registry):
// - SPEC: CSS UI 4 §3.1 — `box-sizing` applies to `min-width`/`max-width`/
//   `min-height`/`max-height` as well as `width`/`height`. Under `border-box`,
//   `min-width: 100px` determines a minimum BORDER box width of 100px; with a
//   specified `width: 100px` (also border box) the min does not bite, so the
//   border box stays 100px and the content box is 100 − padding − border.
//   <https://drafts.csswg.org/css-ui-4/#box-sizing>
// - CLOSEST WPT ORACLE: none available — the ui-dom solver does not run a
//   browser. Gecko projects box-sizing at the computed-style level
//   (servo/components/style/properties + `nsStylePosition` min/max fields);
//   Blink applies it in `ComputedStyle` + `LayoutBox::ComputeMinMaxSizes`.
// - CURRENT (asserted): the sizing clamps read the specified min/max directly
//   — `resolved_content_size_px` (`layout/flow.rs`) calls
//   `clamp_length(..., style.min_width_px, style.max_width_px)` at
//   `flow.rs:1525-1526` / `1540-1541` — interpreting min/max in CONTENT-box
//   terms regardless of `box-sizing`. So `box-sizing: border-box; width: 100px;
//   min-width: 100px` with 10px left + 10px right padding clamps the content
//   box (80px) up to 100px and then adds the 20px insets back: the staged
//   border box is 120px.
// - SPEC-CORRECT: 100px (min-width == the specified border-box width, so it
//   does not bite; content = 80px).
// - This pin fails if the behavior moves EITHER way: a spec-correct fix drops
//   the staged width to 100px (relabel on-spec); an accidental change that
//   stops the min clamp from biting (content stays 80px without the border-box
//   min projection) also moves the pin.
fn off_spec_box_sizing_min_max_sizes_clamp_in_content_box_terms() {
    assert_eq!(
        sized_target_border_box_width(
            ".x { box-sizing: border-box; width: 100px; min-width: 100px; \
             padding-left: 10px; padding-right: 10px }",
        ),
        120.0,
        "the min-width clamp reads the specified value in content-box terms and adds the insets back",
    );

    // CONTROL: the same box-sizing border-box width WITHOUT the min-width is
    // 100px (the on-spec relationship pinned by
    // `box_sizing_border_box_subtracts_insets`) — the min-width is what pushes
    // it to 120px under the current content-box clamp.
    assert_eq!(
        sized_target_border_box_width(
            ".x { box-sizing: border-box; width: 100px; padding-left: 10px; padding-right: 10px }",
        ),
        100.0,
        "the same border-box width without min-width stays 100px",
    );
}

/// Loads `css` on a fresh root with a class-"x" div target, gives the target an
/// inline 40px height so a rect stages, runs one update, and returns the staged
/// rect's border-box width (`size_px[0]`).
fn sized_target_border_box_width(css: &str) -> f32 {
    let (mut world, mut root, target, _report) = styled_target_load(css);
    assert_style_set(&mut root, target, "height", "40px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    assert_eq!(plans.len(), 1, "one document-root layer plan");
    assert_eq!(plans[0].rects.len(), 1, "one rect");
    plans[0].rects[0].size_px[0]
}

#[test]
// UNSUPPORTED (class: unimplemented — D6 admission-gated):
// - SPEC: CSS Sizing 4 §aspect-ratio, CSS Images 3 §object-fit/§object-position
//   — `aspect-ratio` constrains auto sizes; `object-fit`/`object-position`
//   govern replaced-content painting inside the content box.
//   <https://drafts.csswg.org/css-sizing-4/#ratios>
//   <https://drafts.csswg.org/css-images-3/#the-object-fit>
// - CURRENT (asserted): all three drop + count; the img keeps its intrinsic
//   placeholder (160×90, 16:9) and its sizing stays on the intrinsic-ratio
//   path (`layout/replaced_elements.rs`): width 320 → height 180, unchanged by
//   the dropped declarations. NOTE: a bare unloaded img (src set, no authored
//   dimension) resolves to 160×0 — the 90px placeholder height materializes
//   only through the intrinsic-ratio transfer when one dimension is authored.
// - SPEC-CORRECT: `Option<f32>` aspect-ratio field + img paint algorithm;
//   admission per D6.
fn unsupported_replaced_size_hints_drop() {
    assert_decl_drop(
        ".x { aspect-ratio: 16 / 9; object-fit: cover; object-position: center }",
        0,
        3,
    );

    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let img = assert_append_img_to(&mut root, document_root);
    assert_set_attr_str(&mut root, img, "src", "memory://a.png");
    assert_set_attr_str(&mut root, img, "class", "x");
    assert_style_set(&mut root, img, "width", "320px");
    assert_load_root_stylesheet(
        &root,
        theme_stylesheet_origin(),
        ".x { aspect-ratio: 16 / 9; object-fit: cover; object-position: center }",
    );

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    // The placeholder intrinsic stays 160×90 (16:9), and the dropped
    // declarations leave replaced-element sizing on the intrinsic-ratio path.
    let intrinsic = img_intrinsic(&root, img);
    assert_eq!(intrinsic.width_px, 160.0, "placeholder intrinsic width");
    assert_eq!(intrinsic.height_px, 90.0, "placeholder intrinsic height");

    let layout = assert_node_layout(&root, img);
    assert_close(layout.width_px, 320.0, "img used width");
    assert_close(
        layout.height_px,
        180.0,
        "img used height (intrinsic 16:9 ratio)",
    );
}

#[test]
// UNSUPPORTED (class: unimplemented — D6 admission-gated):
// - SPEC: CSS 2.1 §9.5 `float`/`clear` — `float` removes a box from normal
//   flow and shortens line boxes; `clear` moves a box past floats.
//   <https://www.w3.org/TR/CSS21/visuren.html#floats>
// - CURRENT (asserted): `float` and `clear` each drop + count; the second
//   sibling stays in normal block flow directly below the first (block-stacking
//   y positions equal the no-declaration baseline).
// - SPEC-CORRECT: out-of-flow boxes, line shortening, and clear bands in
//   `domains/layout/flow.rs` (the largest B6 item; a likely never-admit);
//   admission per D6.
fn unsupported_float_and_clear_drop() {
    const SHEET: &str = ".a { float: left } .b { clear: both }";
    assert_decl_drop(SHEET, 0, 2);

    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let a = assert_append_div_to(&mut root, document_root);
    assert_set_attr_str(&mut root, a, "class", "a");
    let b = assert_append_div_to(&mut root, document_root);
    assert_set_attr_str(&mut root, b, "class", "b");
    assert_style_set(&mut root, a, "width", "80px");
    assert_style_set(&mut root, a, "height", "20px");
    assert_style_set(&mut root, b, "width", "80px");
    assert_style_set(&mut root, b, "height", "20px");
    assert_load_root_stylesheet(&root, theme_stylesheet_origin(), SHEET);

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    assert_eq!(plans[0].rects.len(), 2, "two sibling rects");
    // Normal block flow: `.b` stays directly below `.a` (float/clear dropped).
    assert_rect_op_geometry(
        &plans[0].rects[0],
        0.0,
        0.0,
        80.0,
        20.0,
        "first sibling (.a)",
    );
    assert_rect_op_geometry(
        &plans[0].rects[1],
        0.0,
        20.0,
        80.0,
        20.0,
        "second sibling (.b) below first",
    );
}

#[test]
// UNSUPPORTED (class: unimplemented — D6 admission-gated):
// - SPEC: CSS Containment 2 §contain; CSS Contain 2 §content-visibility; CSS
//   Fragmentation 3 §break-inside; CSS UI 4 §resize; CSSOM View §scroll-behavior;
//   CSS Scroll Snap 1 §scroll-snap-type; CSS Overscroll Behavior 1
//   §overscroll-behavior; Pointer Events 2 §touch-action; CSS Will Change 1
//   §will-change.
// - CURRENT (asserted): each hint property drops + counts and the update
//   completes without crashing (layout/paint/scroll outcomes equal the
//   no-declaration baseline; scroll behavior is pinned by the existing scroll
//   unit tests).
// - SPEC-CORRECT: per-property admission; gated by D6.
fn unsupported_layout_hint_properties_drop() {
    struct Case {
        name: &'static str,
        body: &'static str,
    }

    let cases = [
        Case {
            name: "contain_layout",
            body: "contain: layout",
        },
        Case {
            name: "content_visibility_auto",
            body: "content-visibility: auto",
        },
        Case {
            name: "break_inside_avoid",
            body: "break-inside: avoid",
        },
        Case {
            name: "resize_both",
            body: "resize: both",
        },
        Case {
            name: "scroll_behavior_smooth",
            body: "scroll-behavior: smooth",
        },
        Case {
            name: "scroll_snap_type_x_mandatory",
            body: "scroll-snap-type: x mandatory",
        },
        Case {
            name: "overscroll_behavior_contain",
            body: "overscroll-behavior: contain",
        },
        Case {
            name: "touch_action_none",
            body: "touch-action: none",
        },
        Case {
            name: "will_change_transform",
            body: "will-change: transform",
        },
    ];

    for case in &cases {
        assert_decl_drop(&format!(".{} {{ {} }}", case.name, case.body), 0, 1);
    }

    // No crash: a combined sheet with every hint drops and the update completes.
    let combined = ".x { contain: layout; content-visibility: auto; break-inside: avoid; \
                    resize: both; scroll-behavior: smooth; scroll-snap-type: x mandatory; \
                    overscroll-behavior: contain; touch-action: none; will-change: transform }";
    let (mut world, mut root, target, report) = styled_target_load(combined);
    assert_eq!(report.declarations_dropped, 9, "all nine hints should drop");
    assert_eq!(report.declarations_applied, 0, "no hint should apply");
    // Inline height (not part of the drop-counted sheet) gives the target a
    // non-zero box so the update stages a paint plan.
    assert_style_set(&mut root, target, "height", "12px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert!(
        !take_staged_layer_paint_plans(&root).is_empty(),
        "update should stage a paint plan",
    );
}

#[test]
#[allow(clippy::too_many_lines)]
// UNSUPPORTED (class: unimplemented — D6 admission-gated):
// - SPEC: CSS Grid 2 §§5–8 — the still-gated grid property family
//   (`grid-template-areas`/`grid-template`, `grid-column`/
//   `grid-row`/`grid-column-start`/`grid-area`, the `grid` shorthand, and the
//   alignment longhands `justify-items`/`justify-self`).
//   <https://drafts.csswg.org/css-grid-2/#grid-properties>
// - CURRENT (asserted): every still-gated grid property name drops + counts at
//   name lookup (the names are absent from the `property_shape` table in
//   `decl_parse.rs`), no crash, and the block-layout baseline is unchanged. The
//   T1 set (`grid-template-columns`/`rows`, `grid-auto-flow`,
//   `grid-auto-columns`/`rows`) left this gate in the grid T0 landing and is
//   pinned admitted in `grid_admission.rs`. CONTROL: the admitted
//   `width: 80px` applies (applied == 1, dropped == 0).
// - SPEC-CORRECT: a grid solver owns these names; admission per the D6 policy.
fn unsupported_grid_property_names_drop() {
    struct Case {
        name: &'static str,
        body: &'static str,
    }

    let cases = [
        Case {
            name: "grid_template_areas",
            body: "grid-template-areas: \"a b\"",
        },
        Case {
            name: "grid_template",
            body: "grid-template: \"a b\" 1fr / 1fr 1fr",
        },
        Case {
            name: "grid_column",
            body: "grid-column: 1 / 2",
        },
        Case {
            name: "grid_row",
            body: "grid-row: 1 / 2",
        },
        Case {
            name: "grid_column_start",
            body: "grid-column-start: 1",
        },
        Case {
            name: "grid_area",
            body: "grid-area: 1 / 1 / 2 / 2",
        },
        Case {
            name: "grid_shorthand",
            body: "grid: \"a b\" 1fr / 1fr 1fr",
        },
        Case {
            name: "justify_items",
            body: "justify-items: center",
        },
        Case {
            name: "justify_self",
            body: "justify-self: center",
        },
    ];

    for case in &cases {
        assert_decl_drop(&format!(".{} {{ {} }}", case.name, case.body), 0, 1);
    }

    // No crash: a combined sheet with every still-gated grid name drops and the
    // update completes, leaving the target on the block-layout baseline. The
    // T1 set (`grid-template-columns`/`rows`, `grid-auto-flow`, `grid-auto-*`)
    // left this gate in the grid T0 landing (see `grid_admission.rs`).
    let combined = ".x { grid-template-areas: \"a b\"; grid-column: 1 / 2; \
                    grid-row: 1 / 2; grid-column-start: 1; grid-area: 1 / 1 / 2 / 2; \
                    justify-items: center; justify-self: center }";
    let (mut world, mut root, target, report) = styled_target_load(combined);
    assert_eq!(
        report.declarations_dropped, 7,
        "all still-gated grid names should drop"
    );
    assert_eq!(
        report.declarations_applied, 0,
        "no still-gated grid name should apply"
    );
    assert_style_set(&mut root, target, "width", "80px");
    assert_style_set(&mut root, target, "height", "40px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(target)
            .expect("target")
            .as_box()
            .display,
        DisplayKw::Block,
        "a div with dropped grid properties stays on the Block display initial",
    );
    let plans = take_staged_layer_paint_plans(&root);
    assert_eq!(plans.len(), 1, "one document-root layer plan");
    assert_eq!(
        plans[0].rects.len(),
        1,
        "one block rect (block-layout baseline)"
    );
    assert_rect_op_geometry(
        &plans[0].rects[0],
        0.0,
        0.0,
        80.0,
        40.0,
        "grid-name-dropped block div",
    );

    // CONTROL: an admitted property still applies alongside a dropped grid name.
    let (mut world, mut root, target, report) =
        styled_target_load(".x { grid-template-areas: \"a b\"; width: 80px }");
    assert_eq!(report.declarations_applied, 1, "the width control applies");
    assert_eq!(report.declarations_dropped, 1, "the grid name drops");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(target).expect("target").as_box().width,
        BoxLength::Px(80.0),
        "width: 80px control should resolve",
    );
}

// — B11: text/effects property tail — //

/// Builds a lone text node under a class-"x" div, optionally loading `css` on
/// the fresh root first, runs one update, and returns the total staged op count
/// across every layer plan (text runs + rects + icons + strokes + svgs +
/// images). Used to prove a dropped decoration adds no paint op.
fn text_tail_op_count(css: Option<&str>) -> usize {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    if let Some(css) = css {
        assert_load_root_stylesheet(&root, theme_stylesheet_origin(), css);
    }
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    assert_set_attr_str(&mut root, container, "class", "x");
    assert_append_text_to(&mut root, container, "Hello world");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    take_staged_layer_paint_plans(&root)
        .iter()
        .map(|plan| {
            plan.rects.len()
                + plan.icons.len()
                + plan.text_runs.len()
                + plan.stroke_paths.len()
                + plan.svgs.len()
                + plan.images.len()
        })
        .sum()
}

/// Builds a backgrounded class-"x" div (a single rect), optionally loading
/// `css` on the fresh root first, runs one update, and returns the total
/// staged rect-op count. Used to prove a dropped `box-shadow` adds no rect.
fn rect_tail_op_count(css: Option<&str>) -> usize {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    if let Some(css) = css {
        assert_load_root_stylesheet(&root, theme_stylesheet_origin(), css);
    }
    let document_root = root.document_node().0;
    let target = assert_append_div_to(&mut root, document_root);
    assert_set_attr_str(&mut root, target, "class", "x");
    assert_style_set(&mut root, target, "background-color", "#336699");
    assert_style_set(&mut root, target, "width", "40px");
    assert_style_set(&mut root, target, "height", "20px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    take_staged_layer_paint_plans(&root)
        .iter()
        .map(|plan| plan.rects.len())
        .sum()
}

#[test]
// UNSUPPORTED (class: unimplemented — D6 admission-gated):
// - SPEC: CSS Text Decoration 4 §2 — `text-decoration` (shorthand) and its
//   longhands `text-decoration-line`/`-color`/`-style`/`-thickness` draw a
//   decoration line over/under/through text runs.
//   <https://drafts.csswg.org/css-text-decor-4/#text-decoration-property>
// - CURRENT (asserted): each name drops + counts (applied == 0, dropped == 1);
//   the shorthand and every longhand are absent from the `property_shape` name
//   table (`decl_parse.rs`), so they drop at name lookup. The no-effect
//   control: a text node under a `text-decoration: underline` container stages
//   the SAME total op count as an unstyled control — one text run, zero rects,
//   no decoration op appears.
// - SPEC-CORRECT: a text-group decoration field (line set + color) with paint
//   emitting a decoration rect per run segment; admission per D6.
fn unsupported_text_decoration_family_drops() {
    for declaration in [
        "text-decoration: underline",
        "text-decoration-line: underline",
        "text-decoration-color: red",
        "text-decoration-style: wavy",
        "text-decoration-thickness: 2px",
    ] {
        assert_decl_drop(&format!(".x {{ {declaration} }}"), 0, 1);
    }

    // No-effect control: the dropped decoration adds no paint op.
    let control = text_tail_op_count(None);
    let styled = text_tail_op_count(Some(".x { text-decoration: underline }"));
    assert_eq!(
        styled, control,
        "text-decoration should leave the staged op count unchanged",
    );
    // One bare-div background rect + one text run; a decoration would add a
    // rect per run segment, so the concrete count pins its absence.
    assert_eq!(
        styled, 2,
        "one bare-div rect + one text run, no decoration op"
    );
}

#[test]
// UNSUPPORTED (class: unimplemented — D6 admission-gated):
// - SPEC: CSS Backgrounds 3 §7.1 `box-shadow` — draws a blurred offset shadow
//   behind the element's box, inside the border box when `inset`.
//   <https://drafts.csswg.org/css-backgrounds-3/#box-shadow>
// - CURRENT (asserted): both forms drop + counts (applied == 0, dropped == 1);
//   `box-shadow` is absent from the `property_shape` name table
//   (`decl_parse.rs`). The no-effect control: a backgrounded box with
//   `box-shadow` stages the SAME rect-op count as the unstyled control — one
//   background rect, no shadow rect appears.
// - SPEC-CORRECT: a bounded shadow list on the visual group with paint emitting
//   blurred/offset rects before the box; admission per D6.
fn unsupported_box_shadow_drops() {
    assert_decl_drop(".x { box-shadow: 2px 3px 4px #000000 }", 0, 1);
    assert_decl_drop(".x { box-shadow: inset 1px 1px 2px #112233 }", 0, 1);

    // No-effect control: the dropped shadow adds no rect.
    let control = rect_tail_op_count(None);
    let styled = rect_tail_op_count(Some(".x { box-shadow: 2px 3px 4px #000000 }"));
    assert_eq!(
        styled, control,
        "box-shadow should leave the staged rect-op count unchanged",
    );
    assert_eq!(styled, 1, "a backgrounded box stages exactly one rect");
}

#[test]
// UNSUPPORTED (class: unimplemented — D6 admission-gated):
// - SPEC: the tail table names belong to CSS Text 4 (`text-transform`,
//   `text-indent`, `text-shadow`, `letter-spacing`, `word-spacing`, `hyphens`),
//   CSS Overflow 3/4 (`text-overflow`, `line-clamp`), CSS Inline Layout 3 +
//   CSS 2.1 §10.8 (`vertical-align`), CSS Writing Modes 4 (`writing-mode`,
//   `unicode-bidi`), CSS Display 3/2.1 §11.2 (`visibility`), CSS UI 4
//   (`cursor`, `user-select`), CSS Lists 3 (`list-style*`), Filter Effects 1
//   (`filter`), Compositing 1 + Filter Effects 2 (`mix-blend-mode`,
//   `backdrop-filter`), CSS Masking 1 (`clip-path`, `mask*`), CSS Images 3
//   (`image-rendering`), CSS Backgrounds 3 §4.2 (`border-style`), CSS 2.1 §17
//   (`border-collapse`, `border-spacing`, `caption-side`, `empty-cells`,
//   `table-layout`), CSS Multi-column 1 (`column-count`, `columns`,
//   `column-rule`), CSS Fragmentation 3 + CSS 2.1 §13.3 (`orphans`, `widows`),
//   and CSS Animations 1 (`animation-name`, `animation-duration`,
//   `animation-timing-function`, `animation-delay`).
// - CURRENT (asserted): every row drops + counts (applied == 0, dropped == 1) —
//   the names are absent from `property_shape` (`decl_parse.rs`), so each
//   declaration drops at name lookup, inert and crash-free.
// - SPEC-CORRECT: per-property admission, gated by D6; the B11 tail table is
//   regenerated from a CSS property index diffed against the 69-name table at
//   each admission.
fn unsupported_property_tail_drops() {
    const TAIL: &[(&str, &str)] = &[
        ("text_transform", "text-transform: uppercase"),
        ("text_indent", "text-indent: 2em"),
        ("text_shadow", "text-shadow: 1px 1px 2px #000"),
        ("text_overflow", "text-overflow: ellipsis"),
        ("letter_spacing", "letter-spacing: 1px"),
        ("word_spacing", "word-spacing: 2px"),
        ("line_clamp", "line-clamp: 2"),
        ("webkit_line_clamp", "-webkit-line-clamp: 2"),
        ("hyphens", "hyphens: auto"),
        ("vertical_align", "vertical-align: middle"),
        ("writing_mode", "writing-mode: vertical-rl"),
        ("unicode_bidi", "unicode-bidi: embed"),
        ("visibility", "visibility: hidden"),
        ("cursor", "cursor: pointer"),
        ("user_select", "user-select: none"),
        ("list_style", "list-style: none"),
        ("list_style_type", "list-style-type: disc"),
        ("list_style_position", "list-style-position: inside"),
        ("filter", "filter: blur(2px)"),
        ("mix_blend_mode", "mix-blend-mode: multiply"),
        ("backdrop_filter", "backdrop-filter: blur(2px)"),
        ("clip_path", "clip-path: inset(0)"),
        ("mask", "mask: none"),
        ("mask_image", "mask-image: url(x.png)"),
        ("image_rendering", "image-rendering: pixelated"),
        ("border_style", "border-style: solid"),
        ("border_collapse", "border-collapse: collapse"),
        ("border_spacing", "border-spacing: 2px"),
        ("caption_side", "caption-side: bottom"),
        ("empty_cells", "empty-cells: hide"),
        ("table_layout", "table-layout: fixed"),
        ("column_count", "column-count: 2"),
        ("columns", "columns: 2"),
        ("column_rule", "column-rule: 1px solid red"),
        ("orphans", "orphans: 2"),
        ("widows", "widows: 2"),
        ("animation_name", "animation-name: spin"),
        ("animation_duration", "animation-duration: 1s"),
        (
            "animation_timing_function",
            "animation-timing-function: ease",
        ),
        ("animation_delay", "animation-delay: 100ms"),
    ];

    for &(name, declaration) in TAIL {
        assert_decl_drop(&format!(".{name} {{ {declaration} }}"), 0, 1);
    }
}

#[test]
// UNSUPPORTED (class: unimplemented — D6 admission-gated):
// - SPEC: CSS Values 4 §`url()`/§`env()`, CSS Values 5 §`attr()`, CSS Lists 3
//   §`counter()`; CSS Images 4 §gradient functions — function-valued
//   declarations whose function form is not admitted.
// - CURRENT (asserted): each row drops + counts exactly once, at one of TWO
//   sites, distinguished here:
//   * name lookup (property unadmitted): `background-image` is absent from
//     `property_shape` (`decl_parse.rs`), so `url(x.png)` and
//     `linear-gradient(red, blue)` never reach a value grammar.
//   * value grammar (unadmitted function on an ADMITTED property): `width` IS
//     admitted, but `attr(data-w)`/`env(safe-area-inset-top)`/`counter(item)`
//     fail the closed `NonNegativeLength` grammar — proven by the CONTROL
//     `.x { width: 10px }`, which applies.
// - SPEC-CORRECT: admit each function form into the relevant grammar;
//   admission per D6.
fn unsupported_value_functions_drop() {
    // name lookup — `background-image` is unadmitted, so the function never
    // reaches a value grammar:
    for &(name, body) in &[
        ("background_image_url", "background-image: url(x.png)"),
        (
            "background_image_linear_gradient",
            "background-image: linear-gradient(red, blue)",
        ),
    ] {
        assert_decl_drop(&format!(".{name} {{ {body} }}"), 0, 1);
    }

    // value grammar — `width` is admitted, but the function form fails the
    // closed length grammar:
    for &(name, body) in &[
        ("width_attr", "width: attr(data-w)"),
        ("width_env", "width: env(safe-area-inset-top)"),
        ("width_counter", "width: counter(item)"),
    ] {
        assert_decl_drop(&format!(".{name} {{ {body} }}"), 0, 1);
    }

    // CONTROL: `width` itself is admitted — a valid length applies (proving the
    // three `width:` rows drop at the value grammar, not the name).
    assert_decl_drop(".x { width: 10px }", 1, 0);
}

#[test]
// UNSUPPORTED (class: unimplemented — D6 admission-gated):
// - SPEC: CSS Content 3 §`content`/§`quotes` (generated content); CSS Lists 3
//   §`counter-reset`/§`counter-increment` (CSS 2.1 §12.4/§12.5 — the counters'
//   governing spec; CSS Counter Styles 3 governs `@counter-style`, not these
//   two properties).
// - CURRENT (asserted): each name drops + counts (applied == 0, dropped == 1);
//   none of `content`, `quotes`, `counter-reset`, `counter-increment` are in
//   the `property_shape` name table (`decl_parse.rs`), so they drop at name
//   lookup.
// - SPEC-CORRECT: a generated-content/counter subsystem; admission per D6.
fn unsupported_content_and_counter_properties_drop() {
    for &(name, declaration) in &[
        ("content", r#"content: "x""#),
        ("quotes", "quotes: auto"),
        ("counter_reset", "counter-reset: item 0"),
        ("counter_increment", "counter-increment: item 1"),
    ] {
        assert_decl_drop(&format!(".{name} {{ {declaration} }}"), 0, 1);
    }
}
