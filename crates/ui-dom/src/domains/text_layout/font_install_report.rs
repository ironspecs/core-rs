//! The consumer-door install report for `ui-dom`.
//!
//! This file owns the plain report types `TextFontCatalog::install_face`
//! returns: a success carrying the allocated face id and the face's family
//! names, or a rejection reason. Both are data-only (no behavior), so they
//! share this file under the structural rules.
//!
//! Invariants and rules:
//!
//! - `install_face` parses boundary bytes with fontdb and never panics on
//!   them: a malformed face is a `Rejected` report, not an `unwrap`
//! - family names are read from the face's name table at install so the
//!   fallback table can route to the face by name

use arrayvec::ArrayVec;

use super::FontFaceId;

/// The outcome of `TextFontCatalog::install_face`: the installed face's
/// identity and family names, or a rejection reason.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FontInstallReport {
    /// The face parsed and was added to the catalog.
    Installed {
        /// The catalog id the face was allocated.
        id: FontFaceId,
        /// The family names read from the face's name table (at most four).
        family_names: ArrayVec<String, 4>,
    },
    /// The bytes were not a usable font and were not added to the catalog.
    Rejected(FontRejectReason),
}

/// Why `install_face` rejected a byte buffer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FontRejectReason {
    /// The bytes did not parse as a font (fontdb found no faces).
    NotAFont,
}
