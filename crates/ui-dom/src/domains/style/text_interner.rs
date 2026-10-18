//! Bounded text arena for interned declaration text in `ui-dom`.
//!
//! This file owns `TextInterner`, the store-relative interner that
//! `TextKey` values resolve against. Text-valued declaration data
//! (font-family, transform, transition lists) is interned here so no
//! per-node `String` copies exist. Grows with the set of distinct authored
//! strings; the eviction/reclamation policy is an open design question
//! (PLAN.md open question 6) — this arena currently never frees.
//!
//! Invariants and rules:
//!
//! - keys are stable indices into `texts`; a key is valid iff it was returned
//!   by `intern` and has not been reclaimed (reclamation never happens today)
//! - `intern` dedups by content and returns the same key for equal strings
//! - `resolve` returns `None` for any out-of-range key, never a panic

use std::collections::HashMap;

use super::decl_value::TextKey;

/// Interns text-valued declaration data (font-family, transform, transition
/// lists) so no per-node string copies exist. Grows with distinct authored
/// strings; eviction policy is an open design question (PLAN.md open
/// question 6).
#[derive(Debug, Default)]
pub struct TextInterner {
    texts: Vec<Box<str>>,
    index: HashMap<Box<str>, u32>,
}

#[allow(dead_code)] // `is_empty` is unused until the wired consumer lands
impl TextInterner {
    pub fn intern(&mut self, text: &str) -> TextKey {
        if let Some(&key) = self.index.get(text) {
            return TextKey(key);
        }
        let boxed: Box<str> = text.into();
        let key = u32::try_from(self.texts.len()).expect("interned text count should fit into u32");
        self.index.insert(boxed.clone(), key);
        self.texts.push(boxed);
        TextKey(key)
    }

    #[must_use]
    pub fn resolve(&self, key: TextKey) -> Option<&str> {
        let index = usize::try_from(key.0).expect("text key should fit into usize");
        self.texts.get(index).map(|text| &**text)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.texts.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.texts.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intern_dedups_same_string_and_resolve_round_trips() {
        let mut interner = TextInterner::default();
        let first = interner.intern("font-family: sans-serif");
        let second = interner.intern("font-family: sans-serif");
        assert_eq!(first, second);
        assert_eq!(interner.len(), 1);
        assert_eq!(interner.resolve(first), Some("font-family: sans-serif"));
    }

    #[test]
    fn distinct_strings_get_distinct_keys() {
        let mut interner = TextInterner::default();
        let first = interner.intern("translate(10px)");
        let second = interner.intern("translateY(10px)");
        assert_ne!(first, second);
        assert_eq!(interner.len(), 2);
        assert!(!interner.is_empty());
    }

    #[test]
    fn resolve_out_of_range_returns_none() {
        let mut interner = TextInterner::default();
        assert_eq!(interner.resolve(TextKey(0)), None);
        assert_eq!(interner.resolve(TextKey(u32::MAX)), None);

        interner.intern("x");
        assert_eq!(interner.resolve(TextKey(0)), Some("x"));
        assert_eq!(interner.resolve(TextKey(1)), None);
    }
}
