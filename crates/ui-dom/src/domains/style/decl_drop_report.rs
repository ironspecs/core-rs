//! Declaration-parse report types for the `ui-dom` style system.
//!
//! This file owns the plain-data result records for drop-and-continue
//! declaration parsing and for one `load_stylesheet` call. They make malformed
//! declarations explicit counts rather than silent swallows.
//!
//! Invariants and rules:
//!
//! - these are plain data: no behavior, no `impl` blocks; callers construct
//!   and read them directly

/// The explicit result of drop-and-continue declaration parsing (CSS 2.1
/// §4.1.7, css-style-attr §3): every malformed declaration is counted, never
/// silently swallowed (the scoped exception to fail-fast parsing, adjudicated
/// in PLAN.md proposal G).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DeclDropReport {
    pub applied: usize,
    pub dropped: usize,
}

/// The explicit result of one `load_stylesheet` call.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StylesheetLoadReport {
    pub rules_inserted: usize,
    pub declarations_applied: usize,
    pub declarations_dropped: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_construct_and_default_to_zero() {
        let drop_report = DeclDropReport::default();
        assert_eq!(drop_report.applied, 0);
        assert_eq!(drop_report.dropped, 0);

        let load_report = StylesheetLoadReport::default();
        assert_eq!(load_report.rules_inserted, 0);
        assert_eq!(load_report.declarations_applied, 0);
        assert_eq!(load_report.declarations_dropped, 0);
    }
}
