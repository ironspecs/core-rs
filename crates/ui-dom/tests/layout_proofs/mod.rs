//! Layout proof integration tests.
//!
//! These tests exercise the public `RootDom` update path with small authored
//! scenarios, then assert the resulting retained layout state. Keep each proof
//! close to one spec claim or one crate-owned layout invariant.

mod block_inline;
mod content_height;
mod fixed_layers;
mod flex_layout;
mod form_control_auto_size;
mod normal_flow;
mod popup_menus;
mod popup_text_rows;
mod replaced_elements;
mod stylesheet_lifecycle;
mod support;
mod text_auto_size;
mod transitions;
mod white_space;
