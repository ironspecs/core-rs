//! Render-owned text-run glyph warmup queue for the retained layer pool.
//!
//! This file owns the preload request queue that warms glyph masks for the
//! retained text-run renderer before first paint. Its purpose is to keep the
//! dedupe-and-batch queue beside its request payloads instead of hiding the
//! policy inside the text-run render state.
//!
//! Invariants and rules:
//!
//! - this file owns only the `PreloadWarmupQueue` struct and its `impl` block
//! - requests are deduped by `PreloadKey` before enqueue
//! - batches drain in FIFO order and release their dedupe keys

use std::collections::HashSet;

use super::layer_pool_text_render::{GenericGlyphWarmupRequest, PreloadKey};

#[derive(Default)]
pub(super) struct PreloadWarmupQueue {
    pub(super) requests: Vec<GenericGlyphWarmupRequest>,
    keys: HashSet<PreloadKey>,
}

impl PreloadWarmupQueue {
    #[cfg(test)]
    pub(super) fn unique_request_count(&self) -> usize {
        self.keys.len()
    }

    pub(super) fn push(&mut self, request: GenericGlyphWarmupRequest) {
        let key = PreloadKey::GenericGlyphWarmup(request.key);
        if !self.keys.insert(key) {
            return;
        }
        self.requests.push(request);
    }

    pub(super) fn take_batch(&mut self, count: usize) -> Vec<GenericGlyphWarmupRequest> {
        let take_count = self.requests.len().min(count);
        let drained: Vec<_> = self.requests.drain(0..take_count).collect();
        for request in &drained {
            let key = PreloadKey::GenericGlyphWarmup(request.key);
            self.keys.remove(&key);
        }
        drained
    }
}
