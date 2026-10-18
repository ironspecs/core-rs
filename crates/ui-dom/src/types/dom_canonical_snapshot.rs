//! Public canonical snapshot for `ui-dom`.
//!
//! This file owns the combined style-plus-layout snapshot that callers persist
//! or replay to reproduce authored style inputs and layout variables together.
//! Its purpose is to keep the wire shape stable without depending on internal
//! node-owned maps.
//!
//! Invariants and rules:
//!
//! - this file owns only the `DomCanonicalSnapshot` struct and its `impl` blocks
//! - serialization stays JSON via `serde_json`

use serde::{Deserialize, Serialize};

use crate::types::{DomCanonicalLayoutSnapshot, DomCanonicalStyleSnapshot};

#[cfg(test)]
use crate::types::{DomAttributeValue, DomIntrinsic, TextIntrinsic};

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
/// Public canonical snapshot for replaying authored style inputs and layout
/// variables together.
pub struct DomCanonicalSnapshot {
    pub style: DomCanonicalStyleSnapshot,
    pub layout: DomCanonicalLayoutSnapshot,
}

impl DomCanonicalSnapshot {
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_snapshot_serializes_to_json_round_trip() {
        let snapshot = DomCanonicalSnapshot {
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
                }),
                available_width_px: 320.0,
                available_height_px: 180.0,
                used_width_px: 120.0,
                used_height_px: 40.0,
            },
        };

        let json = snapshot.to_json().expect("snapshot should serialize");
        let parsed: DomCanonicalSnapshot =
            serde_json::from_str(&json).expect("snapshot should deserialize");
        assert_eq!(parsed, snapshot);
    }
}
