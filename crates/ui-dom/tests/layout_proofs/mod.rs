//! Layout proof integration tests.
//!
//! These tests exercise the public `RootDom` update path with small authored
//! scenarios, then assert the resulting retained layout state. Keep each proof
//! close to one spec claim or one crate-owned layout invariant.

mod layout;
mod pipeline;
mod robustness;
mod style;
mod support;
mod text;
mod widgets;
