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
