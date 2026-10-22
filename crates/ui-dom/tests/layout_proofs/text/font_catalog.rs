//! Font-catalog consumer-door proofs (text seam S3): `install_face` accepts a
//! valid face and rejects garbage, a consumer-installed Arabic face measures and
//! resolves through the public pipeline, and the catalog revision reloads the
//! main-world context at seam entry.
//!
//! The fixture is loaded from disk by these proofs via
//! `env!("CARGO_MANIFEST_DIR")`-relative paths and installed through the public
//! `install_face` API — it is never `include_bytes!`-embedded.
//!
//! Text intrinsics for a fixed face are bit-identical, so the width-difference
//! assertions compare exact `f32` literals (deliberate exact checks).
#![allow(clippy::float_cmp)]

use std::path::Path;

use ui_dom::{
    DomIntrinsic, DomNode, DomNodeSlotKey, DomPointerUpdate, FontFaceId, FontInstallReport,
    FontRejectReason, RootDom, TextFontCatalog,
};

use crate::layout_proofs::support::{
    assert_append_div_to, assert_append_text_to, assert_load_root_stylesheet,
    assert_root_dom_update, assert_set_attr_str, attached_gpu_root_dom_default_viewport_setup,
    theme_stylesheet_origin,
};

/// The OFL Arabic test fixture's on-disk path (never embedded).
fn fixture_path() -> &'static Path {
    Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/fonts/test-fixtures/NotoNaskhArabic-Regular.ttf"
    ))
}

/// Reads the fixture bytes from disk (the consumer door's whole point).
fn fixture_bytes() -> Vec<u8> {
    std::fs::read(fixture_path()).expect("the OFL Arabic fixture should load from disk")
}

/// Installs the fixture through the public door and returns `(id, primary
/// family name)`.
fn install_fixture() -> (FontFaceId, String) {
    let report = TextFontCatalog::repo_default().install_face(&fixture_bytes());
    match report {
        FontInstallReport::Installed { id, family_names } => {
            let family = family_names
                .first()
                .cloned()
                .expect("a valid face should expose at least one family name");
            (id, family)
        }
        FontInstallReport::Rejected(reason) => {
            panic!("the OFL Arabic fixture should install, got {reason:?}");
        }
    }
}

/// Clones one text node's retained intrinsic metrics.
fn text_intrinsic(root: &RootDom, key: DomNodeSlotKey) -> ui_dom::TextIntrinsic {
    match root.node(key).map(DomNode::intrinsic) {
        Some(DomIntrinsic::Text(intrinsic)) => intrinsic.clone(),
        _ => panic!("font-catalog proof text node should keep text intrinsic state"),
    }
}

/// Appends a div (classed) holding one Arabic text node and returns the text
/// node key.
fn append_arabic_text(
    root: &mut RootDom,
    parent: DomNodeSlotKey,
    text: &str,
    class: Option<&str>,
) -> DomNodeSlotKey {
    let container = assert_append_div_to(root, parent);
    if let Some(class) = class {
        assert_set_attr_str(root, container, "class", class);
    }
    assert_append_text_to(root, container, text)
}

#[test]
fn install_face_accepts_a_valid_font() {
    let report = TextFontCatalog::repo_default().install_face(&fixture_bytes());
    let FontInstallReport::Installed { id, family_names } = report else {
        panic!("the OFL Arabic fixture should install as a valid face");
    };
    // The id is a fresh consumer-face id, above the four embedded faces.
    assert!(
        id > FontFaceId::EMBEDDED_SOURCE_HAN_SANS_K,
        "consumer faces should receive ids above the embedded range",
    );
    assert!(
        family_names
            .iter()
            .any(|name| name.contains("Naskh") || name.contains("Amiri")),
        "family names should be read from the face's name table: {family_names:?}",
    );
}

#[test]
fn install_face_rejects_garbage_bytes() {
    let report = TextFontCatalog::repo_default().install_face(b"this is not a font");
    assert_eq!(
        report,
        FontInstallReport::Rejected(FontRejectReason::NotAFont),
        "garbage bytes should be a rejected report, never a panic",
    );
}

#[test]
fn consumer_face_renders_through_the_public_pipeline() {
    // Install the fixture IN the proof (the door's point), then build a styled
    // root whose text node requests the fixture's family.
    let (_id, family) = install_fixture();
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let stylesheet = format!(".arabic {{ font-family: \"{family}\" }}");
    assert_load_root_stylesheet(&root, theme_stylesheet_origin(), &stylesheet);
    let document_root = root.document_node().0;
    let plain = append_arabic_text(&mut root, document_root, "مرحبا بالعالم", None);
    let arabic = append_arabic_text(&mut root, document_root, "مرحبا بالعالم", Some("arabic"));
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plain = text_intrinsic(&root, plain);
    let arabic = text_intrinsic(&root, arabic);

    // (a) The fixture's family measures differently from the default face.
    assert!(
        plain.min_content_width_px != arabic.min_content_width_px
            || plain.max_content_width_px != arabic.max_content_width_px,
        "the fixture's family should measure differently from the default face \
         (default {plain:?} vs fixture {arabic:?})",
    );

    // (b) The seam's resolution routes the fixture's family name to a consumer
    // face (not the language default), reading back the face's own name.
    let (resolved_id, resolved_family) = TextFontCatalog::repo_default()
        .resolve_family(&family)
        .expect("the installed fixture family should resolve");
    assert_eq!(
        resolved_family, family,
        "the resolved family should be the fixture's"
    );
    assert!(
        resolved_id > FontFaceId::EMBEDDED_SOURCE_HAN_SANS_K,
        "the fixture family should resolve to a consumer face, not an embedded one",
    );

    // (c) The shared CATALOG enumerates the consumer face (the single source of
    // truth the render world mirrors). This asserts the CATALOG-side route; the
    // render-world extraction route itself (mint → queue → register → readiness
    // for the consumer `FontFaceId`) is driven by the crate-internal `layer_pool`
    // test `consumer_face_extraction_readies_the_render_world_face`.
    assert!(
        TextFontCatalog::repo_default()
            .installed_face_ids()
            .contains(&resolved_id),
        "the shared catalog should enumerate the consumer face",
    );
}

#[test]
fn catalog_revision_reloads_the_main_world_context() {
    // Warm the thread-local context first: this creates the shared
    // `TextLayoutContext` with the four embedded faces, so the later install
    // reaches it through the revision check rather than a fresh context birth.
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let warm = assert_append_text_to(&mut root, document_root, "warm");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let _ = text_intrinsic(&root, warm);

    // Install the fixture (bumps the catalog revision).
    let (_id, family) = install_fixture();
    let stylesheet = format!(".arabic {{ font-family: \"{family}\" }}");
    assert_load_root_stylesheet(&root, theme_stylesheet_origin(), &stylesheet);

    // A NEW text node (a fresh seam entry) measures in the fixture's family
    // through the SAME context: the revision check re-installs the face with no
    // context rebuild. Compare against a default-face twin.
    let plain = append_arabic_text(&mut root, document_root, "مرحبا بالعالم", None);
    let arabic = append_arabic_text(&mut root, document_root, "مرحبا بالعالم", Some("arabic"));
    assert_root_dom_update(&mut world, &mut root, 0.016, DomPointerUpdate::Unchanged);

    let plain = text_intrinsic(&root, plain);
    let arabic = text_intrinsic(&root, arabic);
    assert!(
        plain.min_content_width_px != arabic.min_content_width_px
            || plain.max_content_width_px != arabic.max_content_width_px,
        "the next seam entry should measure with the installed face \
         (default {plain:?} vs fixture {arabic:?})",
    );
}

#[test]
fn door_2_arabic_shapes_with_real_glyphs() {
    // Door 2 (D5, v3 shape): the Arabic fixture, installed via the consumer
    // API in the proof, proves REAL shaping through the public measure stage —
    // (a) the fixture's metrics are in use (width differs from the default
    // face), and (b) a contextual ligature advances differently from its
    // isolated characters summed, which notdef passthrough cannot produce.
    let (_id, family) = install_fixture();
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let stylesheet = format!(".arabic {{ font-family: \"{family}\" }}");
    assert_load_root_stylesheet(&root, theme_stylesheet_origin(), &stylesheet);
    let document_root = root.document_node().0;

    // ل (U+0644) + ا (U+0627) = the lam-alef ligature. Noto Naskh Arabic
    // substitutes the single lam-alef glyph, whose advance differs from the
    // isolated lam and alef widths summed.
    let pair = append_arabic_text(&mut root, document_root, "لا", Some("arabic"));
    let lam = append_arabic_text(&mut root, document_root, "ل", Some("arabic"));
    let alef = append_arabic_text(&mut root, document_root, "ا", Some("arabic"));
    let plain = append_arabic_text(&mut root, document_root, "لا", None);
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let pair = text_intrinsic(&root, pair);
    let lam = text_intrinsic(&root, lam);
    let alef = text_intrinsic(&root, alef);
    let plain = text_intrinsic(&root, plain);

    // (a) The fixture's family measures the pair differently from the default
    // face (whose Arabic characters fall through to notdef glyphs).
    assert!(
        pair.max_content_width_px != plain.max_content_width_px,
        "the fixture family should measure the Arabic pair differently from the \
         default face (fixture {pair:?} vs default {plain:?})",
    );

    // (b) The lam-alef ligature advances differently from the isolated
    // characters summed — real GSUB shaping, impossible under notdef
    // passthrough (uniform advance, no GSUB engagement).
    assert!(
        pair.max_content_width_px != lam.max_content_width_px + alef.max_content_width_px,
        "the lam-alef ligature should advance differently than the isolated \
         lam + alef summed (pair {:.3}, lam {:.3}, alef {:.3})",
        pair.max_content_width_px,
        lam.max_content_width_px,
        alef.max_content_width_px,
    );
}
