//! Shared DOM-style layout measurement seam for `ui-dom`.
//!
//! This file owns the `DomLayoutResolver` trait that the shared `TreeWalker`
//! calls when it needs an intrinsic node size during a layout walk.
//!
//! Invariants and rules:
//!
//! - this file owns only the `DomLayoutResolver` trait definition
//! - the trait is an object-safe measurement seam with no default method
//!   bodies; concrete resolvers live in their own files

use super::DomLayoutInput;
use crate::types::{DomBoxModel, DomError};

pub trait DomLayoutResolver {
    fn measure_box(&mut self, input: DomLayoutInput<'_>) -> Result<DomBoxModel, DomError>;
}
