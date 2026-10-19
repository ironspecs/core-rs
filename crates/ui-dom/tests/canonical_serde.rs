//! Public serde contract examples for canonical `ui-dom` style and layout data.
//!
//! This file owns fixture-backed tests for the public snapshot types exposed to
//! external callers. Its invariants are:
//!
//! - fixtures represent the public wire shape, not internal engine state
//! - tests here use only exported `ui-dom` snapshot/value types
//! - canonical style stays an ordered declaration list
//! - canonical layout stays limited to intrinsic, available, and used sizes

use ui_dom::{
    DomAttributeValue, DomCanonicalLayoutSnapshot, DomCanonicalSnapshot, DomCanonicalStyleSnapshot,
    DomIntrinsic, DomTag, RootDom, RootDomContext, TextIntrinsic,
};

#[test]
fn canonical_style_snapshot_fixture_matches_public_wire_shape() {
    let fixture = include_str!("fixtures/canonical_serde/style_snapshot.json");
    let snapshot: DomCanonicalStyleSnapshot =
        serde_json::from_str(fixture).expect("style fixture should deserialize");
    let expected = DomCanonicalStyleSnapshot {
        styles: vec![
            (
                "width".to_string(),
                DomAttributeValue::String("14px".to_string()),
            ),
            (
                "color".to_string(),
                DomAttributeValue::Color([1.0, 1.0, 1.0, 1.0]),
            ),
        ],
    };

    assert_eq!(snapshot, expected);

    // The production surface resolves the same list from style_mut writes:
    // `canonical_style_snapshot` serializes the node's inline declarations in
    // block order to the (property-name, value) wire shape.
    let mut root = RootDom::new(RootDomContext::new_cpu());
    let node = root.create_element(DomTag::Div).expect("div");
    root.style_mut(node).set("width", "14px").expect("width");
    root.style_mut(node).set("color", "#ffffff").expect("color");
    assert_eq!(root.canonical_style_snapshot(node), expected);

    let serialized = serde_json::to_value(&snapshot).expect("style snapshot should serialize");
    let fixture_value: serde_json::Value =
        serde_json::from_str(fixture).expect("style fixture value should deserialize");
    assert_eq!(serialized, fixture_value);
}

#[test]
fn canonical_layout_snapshot_fixture_matches_public_wire_shape() {
    let fixture = include_str!("fixtures/canonical_serde/layout_snapshot.json");
    let snapshot: DomCanonicalLayoutSnapshot =
        serde_json::from_str(fixture).expect("layout fixture should deserialize");
    let expected = DomCanonicalLayoutSnapshot {
        intrinsic: DomIntrinsic::Text(TextIntrinsic {
            font_size_px: 16.0,
            min_content_width_px: 24.0,
            max_content_width_px: 48.0,
            line_height_px: 20.0,
            // New fragment-level measurement surface (flex arc step 3a-i):
            // per-edge unbreakable fragment widths, boundary wrap-opportunity
            // flags, and the per-fragment line-height maximum.
            leading_fragment_width_px: 24.0,
            trailing_fragment_width_px: 24.0,
            starts_with_wrap_opportunity: false,
            ends_with_wrap_opportunity: false,
            fragment_line_height_max_px: 20.0,
        }),
        available_width_px: 320.0,
        available_height_px: 180.0,
        used_width_px: 120.0,
        used_height_px: 40.0,
    };

    assert_eq!(snapshot, expected);

    let serialized = serde_json::to_value(&snapshot).expect("layout snapshot should serialize");
    let fixture_value: serde_json::Value =
        serde_json::from_str(fixture).expect("layout fixture value should deserialize");
    assert_eq!(serialized, fixture_value);
}

#[test]
fn canonical_snapshot_fixture_matches_public_wire_shape() {
    let fixture = include_str!("fixtures/canonical_serde/node_snapshot.json");
    let snapshot: DomCanonicalSnapshot =
        serde_json::from_str(fixture).expect("node fixture should deserialize");
    let expected = DomCanonicalSnapshot {
        style: DomCanonicalStyleSnapshot {
            styles: vec![
                (
                    "width".to_string(),
                    DomAttributeValue::String("14px".to_string()),
                ),
                (
                    "color".to_string(),
                    DomAttributeValue::Color([1.0, 1.0, 1.0, 1.0]),
                ),
            ],
        },
        layout: DomCanonicalLayoutSnapshot {
            intrinsic: DomIntrinsic::Text(TextIntrinsic {
                font_size_px: 16.0,
                min_content_width_px: 24.0,
                max_content_width_px: 48.0,
                line_height_px: 20.0,
                // New fragment-level measurement surface (flex arc step 3a-i).
                leading_fragment_width_px: 24.0,
                trailing_fragment_width_px: 24.0,
                starts_with_wrap_opportunity: false,
                ends_with_wrap_opportunity: false,
                fragment_line_height_max_px: 20.0,
            }),
            available_width_px: 320.0,
            available_height_px: 180.0,
            used_width_px: 120.0,
            used_height_px: 40.0,
        },
    };

    assert_eq!(snapshot, expected);

    let serialized = snapshot
        .to_json_pretty()
        .expect("node snapshot should serialize");
    let fixture_value: serde_json::Value =
        serde_json::from_str(fixture).expect("node fixture value should deserialize");
    let serialized_value: serde_json::Value =
        serde_json::from_str(&serialized).expect("serialized node snapshot should deserialize");
    assert_eq!(serialized_value, fixture_value);
}
