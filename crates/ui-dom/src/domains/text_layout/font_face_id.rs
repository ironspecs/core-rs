//! `FontFaceId` — the catalog-scoped identity of one installed font face.
//!
//! This file owns the narrow `FontFaceId` newtype that names a face inside the
//! shared `TextFontCatalog`. It exists so the consumer door (`install_face`)
//! and the render-world font-registration readiness both key on one stable,
//! opaque identity instead of family names or catalog positions.
//!
//! Invariants and rules:
//!
//! - this file owns only the `FontFaceId` struct and its `impl` block
//! - the four embedded faces own the fixed ids `0..4` (catalog order); the
//!   first consumer-installed face is allocated `4` and ids grow monotonically
//! - the id is opaque to consumers: they receive it from `install_face` and
//!   compare it, never construct it from a raw number

use std::fmt;

/// A narrow newtype identifying one face in the shared text font catalog.
///
/// The embedded faces own the four stable low ids (the initial catalog list, by
/// position); consumer-installed faces receive freshly allocated ids from
/// `TextFontCatalog::install_face`. The id is the render-world
/// font-registration key (per-face readiness) and the identity a
/// `FontInstallReport` returns to the caller.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub struct FontFaceId(u32);

impl FontFaceId {
    /// The embedded IBM Plex Sans face (the guaranteed-final fallback face).
    pub const EMBEDDED_IBM_PLEX_SANS: FontFaceId = FontFaceId(0);
    /// The embedded Source Han Sans (Japanese) face.
    pub const EMBEDDED_SOURCE_HAN_SANS_J: FontFaceId = FontFaceId(1);
    /// The embedded Source Han Sans (Simplified Chinese) face.
    pub const EMBEDDED_SOURCE_HAN_SANS_SC: FontFaceId = FontFaceId(2);
    /// The embedded Source Han Sans (Korean) face.
    pub const EMBEDDED_SOURCE_HAN_SANS_K: FontFaceId = FontFaceId(3);

    /// The number of embedded faces; the first id a consumer face receives.
    pub(crate) const EMBEDDED_FACE_COUNT: u32 = 4;

    /// Builds the id for an embedded face at a catalog position. Positions are
    /// `0..EMBEDDED_FACE_COUNT`; the caller asserts the range.
    #[must_use]
    pub(crate) fn embedded_at(index: u32) -> Self {
        assert!(
            index < Self::EMBEDDED_FACE_COUNT,
            "embedded face index should stay in range",
        );
        Self(index)
    }

    /// Allocates a consumer-face id from a raw counter (crate-private).
    #[must_use]
    pub(crate) fn from_raw(raw: u32) -> Self {
        Self(raw)
    }
}

impl fmt::Display for FontFaceId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "FontFaceId({})", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_face_ids_are_stable_and_distinct() {
        assert_eq!(
            FontFaceId::embedded_at(0),
            FontFaceId::EMBEDDED_IBM_PLEX_SANS
        );
        assert_eq!(
            FontFaceId::embedded_at(1),
            FontFaceId::EMBEDDED_SOURCE_HAN_SANS_J,
        );
        assert_eq!(
            FontFaceId::embedded_at(2),
            FontFaceId::EMBEDDED_SOURCE_HAN_SANS_SC,
        );
        assert_eq!(
            FontFaceId::embedded_at(3),
            FontFaceId::EMBEDDED_SOURCE_HAN_SANS_K,
        );
    }

    #[test]
    fn consumer_face_ids_allocate_above_the_embedded_range() {
        let consumer = FontFaceId::from_raw(FontFaceId::EMBEDDED_FACE_COUNT);
        assert_eq!(consumer, FontFaceId::from_raw(4));
        assert_ne!(consumer, FontFaceId::EMBEDDED_IBM_PLEX_SANS);
    }
}
