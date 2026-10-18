#![allow(rustdoc::bare_urls)]
// The style pass resolves px/em lengths to exact `f32` literals and the
// computed groups' initial values are bit-exact, so the typed computed-group
// comparisons in this module are deliberate exact checks.
#![allow(clippy::float_cmp)]

//! Layout-impact property-table absence pins (PLAN.md B6).
//!
//! Purpose: pin the observable behavior of the layout-impact properties that
//! are NOT in the 68-property table (`presentation_property.rs`). Each name (or
//! value) is dropped + counted, inert, and crash-free. Every row is two-sided:
//! the declaration drop is counted (`assert_decl_drop`) AND the defined
//! no-effect holds for its family — sibling paint order stays DOM order
//! (`z-index`), sticky behaves as static, unadmitted display values fall back
//! to the initial display, `float`/`clear` leave block flow stacking unchanged,
//! replaced-element sizing stays on the intrinsic path, and the sizing model is
//! unchanged by `box-sizing`. B11's tail family (`text-decoration`,
//! `box-shadow`, filters, masks, `letter-spacing`, `line-clamp`, …) joins this
//! file later as a separate table family.
//!
//! Class note: every row is `unsupported_*` = unimplemented — policy-gated
//! absence per the D6 admission policy in `MIGRATION.md` (a named consumer
//! need + spec citation + Blink/Gecko algorithm reference + proof). The absent
//! names drop at the name level (`decl_parse.rs`'s `property_shape`); the
//! `position: sticky` and `display: grid`/… rows drop at the VALUE level inside
//! the admitted `position`/`display` grammar.
//!
//! Single-owner note: `overflow-x`/`overflow-y` (B10) and `text-decoration`
//! (B11) are pinned in their owning files and are NOT repeated here.
//!
//! Honesty notes (divergences from PLAN.md B6's stated model, pinned actual):
//! - **`box-sizing` is content-box, not border-box.** PLAN.md B6 says
//!   "border-box is the crate's single sizing model"; the actual sizing
//!   projection (`domains/layout/flow.rs::border_box_width`) computes
//!   `border_box = content + padding + border` — the CSS `content-box` model
//!   (also the CSS initial). `box-sizing: content-box` therefore names the
//!   already-active model and drops with zero sizing change: a `width: 100px`
//!   plus `padding-left/right: 10px` box has a 120px border box, not 100px.
//!   The `unsupported_box_sizing_drops_and_border_box_sizing_holds` row pins
//!   120px.
//! - **A bare unloaded `img` resolves to 160×0, not 160×90.** The 160×90 is
//!   the intrinsic `ImgIntrinsic` placeholder; its 90px height materializes
//!   only through the intrinsic-ratio transfer when one dimension is authored
//!   (`layout/replaced_elements.rs`). The pin asserts the 160×90 intrinsic and
//!   the ratio-transfer baseline (width 320 → height 180) directly.

use bevy::prelude::World;
use ui_dom::{
    DisplayKw, DomIntrinsic, DomNode, DomNodeSlotKey, DomPointerUpdate, ImgIntrinsic, PositionKw,
    RootDom, StylesheetLoadReport,
};

use crate::layout_proofs::support::{
    assert_append_div_to, assert_append_img_to, assert_close, assert_decl_drop,
    assert_load_root_stylesheet, assert_node_layout, assert_rect_op_fill, assert_rect_op_geometry,
    assert_root_dom_update, assert_set_attr_str, assert_style_set,
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
            name: "grid",
            body: "display: grid",
        },
        Case {
            name: "inline_grid",
            body: "display: inline-grid",
        },
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

    // Representative: `display: grid` on a div leaves `DisplayKw::Block` and
    // block layout (the no-declaration baseline).
    let (mut world, mut root, target, _report) = styled_target_load(".x { display: grid }");
    assert_style_set(&mut root, target, "width", "80px");
    assert_style_set(&mut root, target, "height", "40px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_eq!(
        root.computed_style(target)
            .expect("target")
            .as_box()
            .display,
        DisplayKw::Block,
        "display: grid should drop to the Block initial for a div",
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
        "grid-dropped block div",
    );
}

#[test]
// UNSUPPORTED (class: unimplemented — D6 admission-gated):
// - SPEC: CSS UI 4 §box-sizing — `box-sizing: content-box` makes `width`/
//   `height` the CONTENT box, with padding/border added outside.
//   <https://drafts.csswg.org/css-ui-4/#box-sizing>
// - CURRENT (asserted): `box-sizing` drops + counts (width/paddings apply
//   normally), and the crate's single sizing model is CONTENT-box — the
//   authored `width: 100px` is the content box, so the 10px left + 10px right
//   padding expand the border box to **120px**. NOTE: PLAN.md B6's "border-box
//   is the crate's single sizing model" is divergent; the actual projection is
//   `border_box_width = content + padding + border` (`flow.rs`), and
//   `box-sizing: content-box` names that already-active model, so it changes
//   nothing.
// - SPEC-CORRECT: `BoxSizingKw { BorderBox, ContentBox }` with the sizing
//   projection moved into one function; admission per D6.
fn unsupported_box_sizing_drops_and_border_box_sizing_holds() {
    const CSS: &str =
        ".x { box-sizing: content-box; width: 100px; padding-left: 10px; padding-right: 10px }";
    assert_decl_drop(CSS, 3, 1);

    let (mut world, mut root, target, _report) = styled_target_load(CSS);
    // Height comes inline (not part of the spec's declaration block) so the
    // box has a non-zero height and stages a rect.
    assert_style_set(&mut root, target, "height", "40px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    assert_eq!(plans.len(), 1, "one document-root layer plan");
    assert_eq!(plans[0].rects.len(), 1, "one rect");
    // Content-box model: `width: 100px` is the content box; the paddings add
    // 20px of horizontal edges, so the border box is 120px (not 100px).
    assert_rect_op_geometry(
        &plans[0].rects[0],
        0.0,
        0.0,
        120.0,
        40.0,
        "content-box border box (width 100 + padding 20)",
    );
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
