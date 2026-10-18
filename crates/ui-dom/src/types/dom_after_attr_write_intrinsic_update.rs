//! Shared after-attribute-write intrinsic update policy for `ui-dom`.
//!
//! This file owns the enum that declares which attribute writes should
//! invalidate a built-in element's intrinsic size. Its purpose is to keep the
//! authored attribute list explicit instead of scattering the invalidation
//! list across layout or tag-default code.
//!
//! Invariants and rules:
//!
//! - this file owns only the `DomAfterAttrWriteIntrinsicUpdate` enum and its
//!   `impl` blocks
//! - the attribute list is fixed per built-in tag and stays crate-owned

use smallvec::SmallVec;

use crate::types::dom_attribute::DomAttribute;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum DomAfterAttrWriteIntrinsicUpdate {
    #[default]
    None,
    AttributeList(SmallVec<[DomAttribute; 4]>),
}

impl DomAfterAttrWriteIntrinsicUpdate {
    #[must_use]
    pub fn matches(&self, attribute: &DomAttribute) -> bool {
        match self {
            Self::None => false,
            Self::AttributeList(attributes) => {
                attributes.iter().any(|candidate| candidate == attribute)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use smallvec::smallvec;

    #[test]
    fn matches_only_for_intrinsic_relevant_attributes() {
        let none = DomAfterAttrWriteIntrinsicUpdate::None;
        assert!(!none.matches(&DomAttribute::Src));
        assert!(!none.matches(&DomAttribute::Class));

        let relevant = DomAfterAttrWriteIntrinsicUpdate::AttributeList(smallvec![
            DomAttribute::Src,
            DomAttribute::Value,
        ]);
        assert!(relevant.matches(&DomAttribute::Src));
        assert!(relevant.matches(&DomAttribute::Value));
        assert!(!relevant.matches(&DomAttribute::Class));
    }
}
