//! Typed declaration values for the `ui-dom` style system.
//!
//! This file owns the typed value model for parsed declarations: `DeclLength`,
//! `DeclValue`, `TextKey`, and the cascade `Precedence` record. Text values are
//! interned and store-relative: a `TextKey` is an opaque index that is
//! meaningless without the owning store's interner (`TextInterner`), so a text
//! value never carries its own `String` copy on the declaration.
//!
//! Invariants and rules:
//!
//! - `Precedence` field order is the cascade sort order (the cascade tier
//!   first, then origin priority, then the inline flag above any selector
//!   specificity, then specificity, then source order); stages 2–4 of cascade
//!   tier path F extend the `CascadeTier` enum additively without reshaping the
//!   key
//! - `value_equals` is the single documented equality intent for cascade and
//!   upsert change detection; it is plain `PartialEq`, named so callers state
//!   their intent

use super::cascade_tier::CascadeTier;
use super::computed::{
    GridAutoFlowKw, GridLine, GridTemplateAreas, GridTrackList, GridTrackSize, TrackBreadth,
    TrackMaxBreadth,
};
use super::keyword::Keyword;
use crate::types::DomAttributeValue;

#[cfg(test)]
use crate::types::parse_css_color_rgba;

/// A parsed length: the closed `unset | 0 | <px> | <em> | <percentage>`
/// grammar. `Percent` is the declaration form for ALL percentages; resolution
/// is per property (font contexts resolve at style time; box lengths stay
/// symbolic into the group).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DeclLength {
    Unset,
    Px(f32),
    Em(f32),
    Percent(f32),
}

/// Opaque handle to interned text owned by a `DeclStore`; store-relative.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TextKey(pub u32);

slotmap::new_key_type! {
    /// Opaque handle to an interned grid track list owned by a `DeclStore`;
    /// store-relative (value-deduped, so equal lists share one key). `null()`
    /// is the empty-list sentinel (never a live arena entry).
    pub struct GridTrackListKey;
}

slotmap::new_key_type! {
    /// Opaque handle to an interned grid template-areas map owned by a
    /// `DeclStore`; store-relative (value-deduped, so equal maps share one
    /// key). `null()` is the empty-map sentinel (never a live arena entry).
    pub struct GridAreasKey;
}

/// Cascade precedence record for one declaration (field order IS the sort
/// order: the cascade tier first, then origin priority, then the inline-style
/// flag above any selector specificity, then specificity, then source order).
/// The `tier` field projects CSS Cascade 5 §6.1's origin-and-importance ladder
/// (see `cascade_tier.rs`); stages 2–4 of cascade tier path F extend the
/// `CascadeTier` ENUM additively (`ImportantUa` above, `Animation` between
/// `NormalAuthor` and `ImportantAuthor`, layer indices within a tier) rather
/// than reshaping these semantics.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Precedence {
    pub tier: CascadeTier,
    pub origin_priority: i32,
    pub inline: bool,
    pub specificity: super::SelectorSpecificity,
    pub order: u64,
}

/// A parsed declaration value: length, color, keyword, number, integer,
/// interned text, or the typed translate pair.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DeclValue {
    Length(DeclLength),
    Color([f32; 4]),
    Keyword(Keyword),
    Number(f32),
    Integer(i64),
    Text(TextKey),
    /// The scheduler's interpolation form for `transform`. Declarations stay
    /// text-interned (`Keyword(None)` / `Text`) and resolve into the transform
    /// group; this variant is how the group hands the scheduler a typed
    /// `[x, y]` translate pair. It is produced only at group-read time
    /// (`decl_value_for_groups`), never by declaration parse.
    Translate([f32; 2]),
    /// A grid track list, interned value-deduped into the owning store's
    /// track-list arena (`grid-template-columns`/`rows`).
    TrackList(GridTrackListKey),
    /// A grid template-areas map, interned value-deduped into the owning
    /// store's areas arena (`grid-template-areas`).
    GridAreas(GridAreasKey),
    /// A single grid `<track-size>` leaf (`grid-auto-columns`/`rows`).
    TrackSize(GridTrackSize),
    /// A grid auto-flow (`grid-auto-flow`).
    GridAutoFlow(GridAutoFlowKw),
    /// A grid placement line (`grid-column-*`/`grid-row-*`).
    GridLine(GridLine),
}

impl DeclValue {
    /// True when two values are equal for cascade/upsert change detection.
    /// (Plain `PartialEq`; named helper so callers document intent.)
    #[must_use]
    pub fn value_equals(&self, other: &DeclValue) -> bool {
        self == other
    }
}

/// Formats one typed `DeclValue` to its CSS string form. This is the
/// serialization `decl_value_to_dom_value` uses for every kind EXCEPT `Color`
/// (which that function keeps as the typed `DomAttributeValue::Color` wire
/// form). Here `Color` also serializes to its CSS string (`#rrggbb` /
/// `rgba(...)`) so an important declaration can carry a replay-faithful
/// ` !important` suffix through the style-attribute route.
///
/// `resolve_text` resolves the interned text behind `Text`, `Named`, and
/// `NamedSpan` values; it must resolve (the interner and the block live in the
/// same store, so a `Text` key here is a self-owned structural invariant — the
/// `expect` is never on boundary data). `resolve_track_list` resolves a
/// store-owned interned track list, `resolve_areas` a store-owned interned
/// areas map, and `resolve_name` the store-interned line/area NAMES those
/// arena values carry — each likewise a self-owned structural invariant.
#[allow(clippy::too_many_arguments)]
pub(crate) fn decl_value_to_css_string<'a>(
    value: &DeclValue,
    resolve_text: impl Fn(TextKey) -> Option<&'a str>,
    resolve_track_list: impl Fn(GridTrackListKey) -> Option<&'a GridTrackList>,
    resolve_areas: impl Fn(GridAreasKey) -> Option<&'a GridTemplateAreas>,
    resolve_name: impl Fn(TextKey) -> Option<&'a str>,
) -> String {
    match value {
        DeclValue::Length(DeclLength::Unset) => "unset".to_string(),
        DeclValue::Length(DeclLength::Px(value)) => format!("{value}px"),
        DeclValue::Length(DeclLength::Em(value)) => format!("{value}em"),
        DeclValue::Length(DeclLength::Percent(value)) => format!("{value}%"),
        DeclValue::Color(rgba) => format_color_rgba(*rgba),
        DeclValue::Keyword(keyword) => keyword.name().to_string(),
        DeclValue::Number(value) => format!("{value}"),
        DeclValue::Integer(value) => value.to_string(),
        DeclValue::Text(key) => resolve_text(*key)
            .expect("interned text must resolve in its owning store")
            .to_string(),
        DeclValue::Translate([x, y]) => format!("translate({x}px, {y}px)"),
        DeclValue::TrackList(key) => resolve_track_list(*key)
            .expect("interned track list must resolve in its owning store")
            .to_css_string(&resolve_name),
        DeclValue::GridAreas(key) => {
            if *key == GridAreasKey::default() {
                // `grid-template-areas: none` — the empty-map sentinel (the
                // initial value), serialized back to `none`.
                "none".to_string()
            } else {
                format_areas(
                    resolve_areas(*key)
                        .expect("interned areas map must resolve in its owning store"),
                    &resolve_name,
                )
            }
        }
        DeclValue::TrackSize(size) => format_grid_track_size(size),
        DeclValue::GridAutoFlow(flow) => format_grid_auto_flow(*flow).to_string(),
        DeclValue::GridLine(line) => format_grid_line(*line, &resolve_text),
    }
}

/// Serializes a normalized `[red, green, blue, alpha]` color to the CSS string
/// form `parse_css_color_rgba` accepts back: `#rrggbb` when fully opaque, else
/// `rgba(R, G, B, A)` with 0–255 integer channels and the shortest float alpha.
/// Both forms round-trip through `parse_css_color_rgba` for colors it produces
/// (channels are exact `n / 255` and alpha is preserved bit-exactly).
/// Invariant: `DeclValue::Color` channels are parser-normalized to `[0, 1]` at
/// admission (`parse_css_color_rgba`), which is what makes the
/// `f32_to_u8(...).expect(...)` below a self-owned structural invariant rather
/// than a boundary-data panic.
#[allow(clippy::float_cmp)] // the opaque test is an exact 1.0 alpha comparison
fn format_color_rgba(rgba: [f32; 4]) -> String {
    let [red, green, blue, alpha] = rgba;
    let r = crate::numeric::f32_to_u8((red * 255.0).round());
    let g = crate::numeric::f32_to_u8((green * 255.0).round());
    let b = crate::numeric::f32_to_u8((blue * 255.0).round());
    if alpha == 1.0 {
        format!("#{r:02x}{g:02x}{b:02x}")
    } else {
        format!("rgba({r}, {g}, {b}, {alpha})")
    }
}

/// Projects one typed `DeclValue` to its public wire `DomAttributeValue`.
/// `resolve_text` resolves store-owned interned text; it must resolve (the
/// interner and the block live in the same store, so a `Text` key here is a
/// self-owned structural invariant — the `expect` is never on boundary data).
/// `resolve_track_list`/`resolve_areas`/`resolve_name` resolve the
/// store-interned arena values and their names, likewise.
#[allow(clippy::too_many_arguments)]
pub(crate) fn decl_value_to_dom_value<'a>(
    value: &DeclValue,
    resolve_text: impl Fn(TextKey) -> Option<&'a str>,
    resolve_track_list: impl Fn(GridTrackListKey) -> Option<&'a GridTrackList>,
    resolve_areas: impl Fn(GridAreasKey) -> Option<&'a GridTemplateAreas>,
    resolve_name: impl Fn(TextKey) -> Option<&'a str>,
) -> DomAttributeValue {
    match value {
        // `Color` keeps its typed wire form (the serde fixture pins it); the
        // CSS string form for colors lives in `decl_value_to_css_string`.
        DeclValue::Color(rgba) => DomAttributeValue::Color(*rgba),
        _ => DomAttributeValue::String(decl_value_to_css_string(
            value,
            resolve_text,
            resolve_track_list,
            resolve_areas,
            resolve_name,
        )),
    }
}

/// Formats one grid template-areas map as its authored string-row form
/// (`"a a" "b c"`), dot-runs re-emitted as `.` and names resolved through
/// `resolve`. Debug/wire form only — the typed map is the retained truth.
fn format_areas<'a>(
    areas: &GridTemplateAreas,
    resolve: &impl Fn(TextKey) -> Option<&'a str>,
) -> String {
    let mut out = String::new();
    for row in 0..usize::from(areas.row_count) {
        if row > 0 {
            out.push(' ');
        }
        out.push('"');
        for col in 0..usize::from(areas.col_count) {
            if col > 0 {
                out.push(' ');
            }
            match areas.cells[row][col] {
                Some(key) => out.push_str(
                    resolve(key).expect("interned area name must resolve in its owning store"),
                ),
                None => out.push('.'),
            }
        }
        out.push('"');
    }
    out
}

/// Formats one grid `<track-size>` to its canonical CSS token. A plain track
/// (matching min and max breadths) formats as its single keyword; a
/// `minmax(a, b)` whose bounds differ formats as `minmax(a, b)`; an `fr` max
/// formats as `Nfr` (correction 3 — percent bounds format as `"%"`, never the
/// former hardcoded `"px"`).
pub(crate) fn format_grid_track_size(size: &GridTrackSize) -> String {
    if let TrackMaxBreadth::Fr(fr) = size.max {
        return format!("{fr}fr");
    }
    match (size.min, size.max) {
        (TrackBreadth::Auto, TrackMaxBreadth::Auto) => "auto".to_string(),
        (TrackBreadth::MinContent, TrackMaxBreadth::MinContent) => "min-content".to_string(),
        (TrackBreadth::MaxContent, TrackMaxBreadth::MaxContent) => "max-content".to_string(),
        (TrackBreadth::Px(min), TrackMaxBreadth::Px(max)) if min.to_bits() == max.to_bits() => {
            format!("{min}px")
        }
        (TrackBreadth::Percent(min), TrackMaxBreadth::Percent(max))
            if min.to_bits() == max.to_bits() =>
        {
            format!("{min}%")
        }
        _ => format!(
            "minmax({}, {})",
            format_track_breadth(size.min),
            format_track_max_breadth(size.max)
        ),
    }
}

/// Formats one `TrackBreadth` to its canonical CSS token.
fn format_track_breadth(breadth: TrackBreadth) -> String {
    match breadth {
        TrackBreadth::Auto => "auto".to_string(),
        TrackBreadth::Px(px) => format!("{px}px"),
        TrackBreadth::Percent(percent) => format!("{percent}%"),
        TrackBreadth::MinContent => "min-content".to_string(),
        TrackBreadth::MaxContent => "max-content".to_string(),
    }
}

/// Formats one `TrackMaxBreadth` to its canonical CSS token.
fn format_track_max_breadth(breadth: TrackMaxBreadth) -> String {
    match breadth {
        TrackMaxBreadth::Auto => "auto".to_string(),
        TrackMaxBreadth::Px(px) => format!("{px}px"),
        TrackMaxBreadth::Percent(percent) => format!("{percent}%"),
        TrackMaxBreadth::MinContent => "min-content".to_string(),
        TrackMaxBreadth::MaxContent => "max-content".to_string(),
        TrackMaxBreadth::Fr(fr) => format!("{fr}fr"),
    }
}

/// Formats one `GridAutoFlowKw` to its canonical CSS keyword form.
fn format_grid_auto_flow(flow: GridAutoFlowKw) -> &'static str {
    match flow {
        GridAutoFlowKw::Row => "row",
        GridAutoFlowKw::RowDense => "row dense",
        GridAutoFlowKw::Column => "column",
        GridAutoFlowKw::ColumnDense => "column dense",
    }
}

/// Formats one `GridLine` to its canonical CSS token form (`auto`, an integer
/// line, `span <positive-integer>`, a named line, or `span <name> [n]`).
/// `resolve` maps a `TextKey` name to its string (a self-owned structural
/// invariant — a name key must resolve in its owning store). Debug/wire form
/// only — the typed line is the retained truth.
fn format_grid_line<'a>(line: GridLine, resolve: &impl Fn(TextKey) -> Option<&'a str>) -> String {
    match line {
        GridLine::Auto => "auto".to_string(),
        GridLine::Line(n) => n.to_string(),
        GridLine::Span(n) => format!("span {n}"),
        GridLine::Named(key) => resolve(key)
            .expect("interned line name must resolve in its owning store")
            .to_string(),
        GridLine::NamedSpan(key, 0) => format!(
            "span {}",
            resolve(key).expect("interned line name must resolve in its owning store")
        ),
        GridLine::NamedSpan(key, n) => format!(
            "span {} {n}",
            resolve(key).expect("interned line name must resolve in its owning store")
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_equals_matches_across_variants() {
        assert!(
            DeclValue::Length(DeclLength::Px(10.0))
                .value_equals(&DeclValue::Length(DeclLength::Px(10.0)))
        );
        assert!(
            !DeclValue::Length(DeclLength::Px(10.0))
                .value_equals(&DeclValue::Length(DeclLength::Em(10.0)))
        );
        assert!(
            !DeclValue::Length(DeclLength::Px(10.0))
                .value_equals(&DeclValue::Length(DeclLength::Unset))
        );
        assert!(
            DeclValue::Length(DeclLength::Percent(50.0))
                .value_equals(&DeclValue::Length(DeclLength::Percent(50.0)))
        );
        assert!(
            !DeclValue::Length(DeclLength::Percent(50.0))
                .value_equals(&DeclValue::Length(DeclLength::Px(50.0)))
        );
        assert!(DeclValue::Keyword(Keyword::Auto).value_equals(&DeclValue::Keyword(Keyword::Auto)));
        assert!(
            !DeclValue::Keyword(Keyword::Auto).value_equals(&DeclValue::Keyword(Keyword::Hidden))
        );
        assert!(DeclValue::Number(1.5).value_equals(&DeclValue::Number(1.5)));
        assert!(DeclValue::Integer(-3).value_equals(&DeclValue::Integer(-3)));
        assert!(!DeclValue::Integer(3).value_equals(&DeclValue::Number(3.0)));
        assert!(DeclValue::Text(TextKey(7)).value_equals(&DeclValue::Text(TextKey(7))));
        assert!(!DeclValue::Text(TextKey(7)).value_equals(&DeclValue::Text(TextKey(8))));
        assert!(
            DeclValue::Color([1.0, 0.0, 0.0, 1.0])
                .value_equals(&DeclValue::Color([1.0, 0.0, 0.0, 1.0]))
        );
        assert!(DeclValue::Translate([3.0, 4.0]).value_equals(&DeclValue::Translate([3.0, 4.0])));
        assert!(!DeclValue::Translate([3.0, 4.0]).value_equals(&DeclValue::Translate([4.0, 3.0])));
    }

    #[test]
    #[allow(clippy::too_many_lines)] // every DeclValue kind is a row in this projection
    fn decl_value_to_dom_value_formats_every_decl_value_kind() {
        let resolve_none = |_key: TextKey| -> Option<&str> { None };
        let resolve_text = |_key: TextKey| -> Option<&str> { Some("ui-sans, sans-serif") };
        let no_list = |_key: GridTrackListKey| -> Option<&GridTrackList> { None };
        let no_areas = |_key: GridAreasKey| -> Option<&GridTemplateAreas> { None };

        assert_eq!(
            dom(
                &DeclValue::Length(DeclLength::Unset),
                resolve_none,
                no_list,
                no_areas
            ),
            DomAttributeValue::String("unset".to_string())
        );
        assert_eq!(
            dom(
                &DeclValue::Length(DeclLength::Px(20.0)),
                resolve_none,
                no_list,
                no_areas
            ),
            DomAttributeValue::String("20px".to_string())
        );
        assert_eq!(
            dom(
                &DeclValue::Length(DeclLength::Px(0.0)),
                resolve_none,
                no_list,
                no_areas
            ),
            DomAttributeValue::String("0px".to_string())
        );
        assert_eq!(
            dom(
                &DeclValue::Length(DeclLength::Px(-10.0)),
                resolve_none,
                no_list,
                no_areas
            ),
            DomAttributeValue::String("-10px".to_string())
        );
        assert_eq!(
            dom(
                &DeclValue::Length(DeclLength::Px(0.5)),
                resolve_none,
                no_list,
                no_areas
            ),
            DomAttributeValue::String("0.5px".to_string())
        );
        assert_eq!(
            dom(
                &DeclValue::Length(DeclLength::Em(1.5)),
                resolve_none,
                no_list,
                no_areas
            ),
            DomAttributeValue::String("1.5em".to_string())
        );
        assert_eq!(
            dom(
                &DeclValue::Length(DeclLength::Percent(50.0)),
                resolve_none,
                no_list,
                no_areas
            ),
            DomAttributeValue::String("50%".to_string())
        );
        assert_eq!(
            dom(
                &DeclValue::Length(DeclLength::Percent(-10.0)),
                resolve_none,
                no_list,
                no_areas
            ),
            DomAttributeValue::String("-10%".to_string())
        );
        assert_eq!(
            dom(
                &DeclValue::Color([1.0, 0.0, 0.0, 0.5]),
                resolve_none,
                no_list,
                no_areas
            ),
            DomAttributeValue::Color([1.0, 0.0, 0.0, 0.5])
        );
        assert_eq!(
            dom(
                &DeclValue::Keyword(Keyword::Justified),
                resolve_none,
                no_list,
                no_areas
            ),
            DomAttributeValue::String("justify".to_string())
        );
        assert_eq!(
            dom(
                &DeclValue::Keyword(Keyword::RowReverse),
                resolve_none,
                no_list,
                no_areas
            ),
            DomAttributeValue::String("row-reverse".to_string())
        );
        assert_eq!(
            dom(&DeclValue::Number(2.0), resolve_none, no_list, no_areas),
            DomAttributeValue::String("2".to_string())
        );
        assert_eq!(
            dom(&DeclValue::Number(0.5), resolve_none, no_list, no_areas),
            DomAttributeValue::String("0.5".to_string())
        );
        assert_eq!(
            dom(&DeclValue::Integer(-3), resolve_none, no_list, no_areas),
            DomAttributeValue::String("-3".to_string())
        );
        assert_eq!(
            dom(
                &DeclValue::Text(TextKey(7)),
                resolve_text,
                no_list,
                no_areas
            ),
            DomAttributeValue::String("ui-sans, sans-serif".to_string())
        );
        assert_eq!(
            dom(
                &DeclValue::Translate([5.0, 0.0]),
                resolve_none,
                no_list,
                no_areas
            ),
            DomAttributeValue::String("translate(5px, 0px)".to_string())
        );
        assert_eq!(
            dom(
                &DeclValue::Translate([0.0, -3.5]),
                resolve_none,
                no_list,
                no_areas
            ),
            DomAttributeValue::String("translate(0px, -3.5px)".to_string())
        );
    }

    /// The shared five-resolver formatter call, with the name resolver defaulting
    /// to the text resolver (the call sites under test carry no arena names).
    fn dom<'a>(
        value: &DeclValue,
        resolve_text: impl Fn(TextKey) -> Option<&'a str>,
        resolve_track_list: impl Fn(GridTrackListKey) -> Option<&'a GridTrackList>,
        resolve_areas: impl Fn(GridAreasKey) -> Option<&'a GridTemplateAreas>,
    ) -> DomAttributeValue {
        decl_value_to_dom_value(
            value,
            resolve_text,
            resolve_track_list,
            resolve_areas,
            |_key| None,
        )
    }

    #[test]
    fn grid_areas_none_serializes_back_to_none() {
        // `grid-template-areas: none` stores the null key (the empty-map
        // sentinel); the wire formatter serializes it back to `none` instead of
        // resolving the (deliberately unresolvable) sentinel.
        let resolve_none = |_key: TextKey| -> Option<&str> { None };
        let no_list = |_key: GridTrackListKey| -> Option<&GridTrackList> { None };
        let no_areas = |_key: GridAreasKey| -> Option<&GridTemplateAreas> { None };
        assert_eq!(
            dom(
                &DeclValue::GridAreas(GridAreasKey::default()),
                resolve_none,
                no_list,
                no_areas,
            ),
            DomAttributeValue::String("none".to_string())
        );
    }

    #[test]
    #[allow(clippy::float_cmp)] // round-trip equality is bit-exact by contract
    fn format_color_rgba_round_trips_through_the_color_parser() {
        // Opaque colors serialize to 6-digit hex and round-trip bit-exactly.
        let opaque = parse_css_color_rgba("#112233").expect("#112233");
        assert_eq!(format_color_rgba(opaque), "#112233");
        assert_eq!(
            parse_css_color_rgba(&format_color_rgba(opaque)).expect("round-trip"),
            opaque,
        );

        // Non-opaque colors serialize to rgba(R, G, B, A) with 0-255 integer
        // channels and the shortest float alpha; the form round-trips too.
        let translucent = parse_css_color_rgba("rgba(255, 128, 0, 0.25)").expect("rgba");
        assert_eq!(format_color_rgba(translucent), "rgba(255, 128, 0, 0.25)");
        assert_eq!(
            parse_css_color_rgba(&format_color_rgba(translucent)).expect("round-trip"),
            translucent,
        );
    }

    #[test]
    fn decl_value_to_css_string_formats_color_as_css_text() {
        let resolve_none = |_key: TextKey| -> Option<&str> { None };
        assert_eq!(
            decl_value_to_css_string(
                &DeclValue::Color([0.0, 0.0, 0.0, 1.0]),
                resolve_none,
                |_key| None,
                |_key| None,
                |_key| None,
            ),
            "#000000",
        );
        assert_eq!(
            decl_value_to_css_string(
                &DeclValue::Color([1.0, 0.0, 0.0, 0.5]),
                resolve_none,
                |_key| None,
                |_key| None,
                |_key| None,
            ),
            "rgba(255, 0, 0, 0.5)",
        );
    }

    #[test]
    fn grid_line_formats_to_its_css_token_form() {
        let resolve = |_key: TextKey| -> Option<&str> { Some("main") };
        assert_eq!(format_grid_line(GridLine::Auto, &|_| None), "auto");
        assert_eq!(format_grid_line(GridLine::Line(2), &|_| None), "2");
        assert_eq!(format_grid_line(GridLine::Line(-1), &|_| None), "-1");
        assert_eq!(format_grid_line(GridLine::Span(3), &|_| None), "span 3");
        assert_eq!(
            format_grid_line(GridLine::Named(TextKey(0)), &resolve),
            "main"
        );
        assert_eq!(
            format_grid_line(GridLine::NamedSpan(TextKey(0), 0), &resolve),
            "span main"
        );
        assert_eq!(
            format_grid_line(GridLine::NamedSpan(TextKey(0), 3), &resolve),
            "span main 3"
        );
    }
}
