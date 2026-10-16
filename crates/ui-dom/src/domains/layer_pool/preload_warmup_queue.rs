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

#[cfg(test)]
use super::layer_pool_text_render::{GenericGlyphWarmupKey, GenericWarmupKind};
#[cfg(test)]
use crate::domains::layer_ops::{TextFontFamilyId, TextLanguageId};

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

#[cfg(test)]
mod tests {
    use super::*;

    fn warmup_request(sample_index: u16, sample_text: &'static str) -> GenericGlyphWarmupRequest {
        GenericGlyphWarmupRequest {
            key: GenericGlyphWarmupKey {
                font_family_id: TextFontFamilyId::UiSans,
                language_hint_id: TextLanguageId::English,
                font_size_bits: 0,
                raster_scale_bits: 0,
                warmup_kind: GenericWarmupKind::GlyphMask,
                sample_index,
            },
            sample_text,
        }
    }

    #[test]
    fn push_dedups_by_key_and_take_batch_releases_keys_fifo() {
        let mut queue = PreloadWarmupQueue::default();
        let request_one = warmup_request(0, "one");
        let request_two = warmup_request(1, "two");

        queue.push(request_one);
        queue.push(request_one);
        assert_eq!(queue.unique_request_count(), 1);

        queue.push(request_two);
        assert_eq!(queue.unique_request_count(), 2);

        let first = queue.take_batch(1);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].sample_text, "one");
        assert_eq!(first[0].key.sample_index, 0);

        let rest = queue.take_batch(8);
        assert_eq!(rest.len(), 1);
        assert_eq!(rest[0].sample_text, "two");

        assert!(queue.take_batch(8).is_empty());

        queue.push(request_one);
        assert_eq!(queue.unique_request_count(), 1);
    }
}
