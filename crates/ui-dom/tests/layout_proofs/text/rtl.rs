//! Text-seam door 3 (RTL) public proof: the observable RTL behavior through
//! the public pipeline.
//!
//! The run-level `rtl` flag and the caret-map stop ordering are crate-internal
//! (see the door-3 pins in `src/domains/text_layout/mod.rs`): no public type
//! exposes cosmic-text's `LayoutRun.rtl`, and the public caret route (the
//! focused-input caret rect) exposes a single collapsed-selection caret x, not
//! the full per-run stop ordering. The door's PUBLIC proof is the observable
//! behavior — the bidi "position effect": `direction: rtl` governs
//! `text-align: start/end` resolution (CSS Writing Modes 4 §direction), so an
//! RTL node's default `start` alignment resolves to physical RIGHT in the
//! staged text-run op, with the OFL Arabic fixture in use.
//!
//! The fixture is loaded from disk via the public `install_face` door — never
//! embedded (the door's whole point).

use std::path::Path;

use ui_dom::{DomPointerUpdate, FontInstallReport, TextAlign, TextFontCatalog};

use crate::layout_proofs::support::{
    assert_append_div_to, assert_append_text_to, assert_load_root_stylesheet,
    assert_root_dom_update, assert_set_attr_str, attached_gpu_root_dom_default_viewport_setup,
    take_staged_layer_paint_plans, theme_stylesheet_origin,
};

/// The OFL Arabic test fixture's on-disk path (never embedded).
fn fixture_path() -> &'static Path {
    Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/fonts/test-fixtures/NotoNaskhArabic-Regular.ttf"
    ))
}

/// Installs the fixture through the public door and returns its primary family
/// name.
fn install_fixture() -> String {
    let bytes =
        std::fs::read(fixture_path()).expect("the OFL Arabic fixture should load from disk");
    match TextFontCatalog::repo_default().install_face(&bytes) {
        FontInstallReport::Installed { family_names, .. } => family_names
            .first()
            .cloned()
            .expect("a valid face should expose at least one family name"),
        FontInstallReport::Rejected(reason) => {
            panic!("the OFL Arabic fixture should install, got {reason:?}");
        }
    }
}

#[test]
fn door_3_rtl_direction_governs_start_alignment() {
    // The door-3 public proof: `direction` governs start/end alignment. With
    // the Arabic fixture installed and routed via `font-family`, a
    // `direction: rtl` text run carries `TextAlign::Right` for the default
    // `start` alignment (the physical right side), while `direction: ltr`
    // carries `TextAlign::Left` — the bidi position effect, observed through
    // the staged paint plan's text-run op.
    let family = install_fixture();
    for (name, direction, expected) in [
        ("rtl", "rtl", TextAlign::Right),
        ("ltr_control", "ltr", TextAlign::Left),
    ] {
        let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
        let stylesheet = format!(".t {{ direction: {direction}; font-family: \"{family}\" }}");
        assert_load_root_stylesheet(&root, theme_stylesheet_origin(), &stylesheet);
        let document_root = root.document_node().0;
        let container = assert_append_div_to(&mut root, document_root);
        assert_set_attr_str(&mut root, container, "class", "t");
        assert_append_text_to(&mut root, container, "مرحبا بالعالم");
        assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

        let plans = take_staged_layer_paint_plans(&root);
        assert_eq!(plans.len(), 1, "{name} should stage one layer plan");
        assert_eq!(
            plans[0].text_runs.len(),
            1,
            "{name} should stage one text run",
        );
        assert_eq!(
            plans[0].text_runs[0].alignment, expected,
            "{name} text run should carry {expected:?} (direction governs start/end)",
        );
    }
}
