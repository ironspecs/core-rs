#![allow(rustdoc::bare_urls)]

//! Keyword-grammar case-sensitivity survey (the
//! `off_spec_keyword_grammars_are_case_sensitive` divergent row).
//!
//! CSS Syntax 3 §4.1 makes CSS keywords ASCII case-insensitive (all CSS syntax
//! is case-insensitive within the ASCII range). The crate's closed keyword
//! grammars match case-sensitively — `Keyword::from_name_in` compares
//! `keyword.name() == value` exactly (`keyword.rs`), and the grid `<flex>`
//! unit is a case-sensitive `"fr"` suffix match (`decl_parse.rs`
//! `parse_track_size`) — so an uppercase keyword DROPS where its lowercase
//! spelling applies. This file pins that ACTUAL behavior per representative
//! family as a running proof. Each row is two-sided: the uppercase form drops
//! and is counted, while the lowercase control applies, so the pinned
//! divergence is case-sensitivity, not keyword absence.
//!
//! Admission of ASCII case-insensitive keyword matching is deferred per D6 (no
//! named consumer need). The one admitted exception is the
//! `grid-template-areas: none` initial-value entry point, which matches ASCII
//! case-insensitively by design (FIX 2) and is pinned by
//! `grid_template_areas_none_resets_to_the_empty_map` in `grid_admission.rs`.

use crate::layout_proofs::support::assert_decl_drop;

#[test]
fn off_spec_keyword_grammars_are_case_sensitive() {
    // (family, property, uppercase form, lowercase control)
    let cases: [(&str, &str, &str, &str); 5] = [
        ("display", "display", "GRID", "grid"),
        ("flex-direction", "flex-direction", "ROW", "row"),
        ("position", "position", "ABSOLUTE", "absolute"),
        ("white-space", "white-space", "PRE", "pre"),
        ("grid <flex> unit", "grid-template-columns", "1FR", "1fr"),
    ];

    for (_family, property, upper, lower) in cases {
        // The uppercase form drops + counts: the keyword grammar matched
        // case-sensitively, so `{_family}: {upper}` is not admitted. The drop
        // assertion labels the failure with the exact declaration text, which
        // names the row.
        assert_decl_drop(&format!(".x {{ {property}: {upper} }}"), 0, 1);
        // The lowercase control applies: the keyword is otherwise admitted,
        // so the divergence is case-sensitivity, not keyword absence.
        assert_decl_drop(&format!(".x {{ {property}: {lower} }}"), 1, 0);
    }
}
