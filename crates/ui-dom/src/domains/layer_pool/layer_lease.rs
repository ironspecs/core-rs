//! `LayerLease` is the retained layer token that identifies one live `LayerPool` slot.
//! This file exists so callers can hold a strict capability without gaining access to retained images or mutable layer bookkeeping.
//!
//! The token stays tiny and copyable because it crosses the authored-DOM to retained-layer boundary often.
//! Its internals stay `pub(super)` so only the owning layer-pool module can validate generations or resolve slot ids.
//!
//! Strict invariants:
//! - `LayerLease` is identification only, not ownership of a retained surface.
//! - `LayerLease` must not expose raw retained image handles or retained mapping fields.
//! - a lease is valid only when its generation still matches the current slot generation.

use super::{LayerGeneration, LayerId};

/// Retained token for one logical layer entry in `LayerPool`.
///
/// This token does not own the retained surface and does not expose retained
/// image handles or retained mapping details.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LayerLease {
    pub(super) layer_id: LayerId,
    pub(super) generation: LayerGeneration,
}
