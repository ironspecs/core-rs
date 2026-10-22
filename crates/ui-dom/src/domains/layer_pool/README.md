# layer_pool

The retained layer-pool boundary for `ui-dom`: retained surface ownership,
render execution support, and the render-owned text-run state that rasterizes
and caches glyphs — the rasterize/cache stage of the text seam.

## Purpose and scope

This module owns retained layer ownership (`LayerPool`), render-world
execution (`LayerPoolRenderExecutor`, `LayerPoolRenderState`), and the
render-owned side of the text seam: FontSystem #2, the swash glyph-mask cache,
the retained text-buffer LRU, font registration, and glyph-mask warmup. It
owns the GPU-side rasterization and cache that `domains/text_layout` feeds
with laid-out `Buffer`s. It does not own text measurement, line breaking, or
caret layout (that is `domains/text_layout`); it does not own the staged paint
ops themselves (that is `domains/render_ops` and `domains/paint`).

## Ownership boundaries

- `layer_pool.rs` owns `LayerPool`: the retained backing-image allocator, the
  retained float-to-int mapping, the canonical text-run registry
  (`SlotMap<TextRunId, CanonicalTextRunRecord>` keyed by `TextRunSignature`),
  and the retained layer bookkeeping.
- `layer_pool_render_executor.rs` owns the render-world executor: it drains
  pending font loads, required-asset declarations, and text-run changes into
  `TextRunRenderState`, then applies staged plans to surfaces.
- `layer_pool_render_state.rs` owns `LayerPoolRenderState` (the five pipeline
  states: rect, icon, text-run, svg, image).
- `text_run_render_state.rs` owns `TextRunRenderState`: FontSystem #2, the
  `SwashCache`, the run bank, the text-buffer LRU, the glyph-mask LRU, the
  preload warmup queue, and the pending lease-dependencies map.
- `layer_pool_text_render.rs` owns the render text-run helpers: `build_text_buffer`
  (which builds a `TextLayoutRequest` and calls the seam's
  `text_layout::text_render::build_render_text_buffer`), `build_glyph_mask`,
  the debug-rect demotion decision, and the GPU text-run submission.
- `font_registration_state.rs` owns `FontRegistrationState` (required asset ids
  per FACE, keyed by `FontFaceId`, vs registered asset ids);
  `preload_warmup_queue.rs` owns `PreloadWarmupQueue` (deduped FIFO
  glyph-warmup batches).

## The rasterize/cache stage's side of the seam

The text seam's third stage lives here. `domains/text_layout` produces laid-out
`Buffer`s and glyph runs; this module rasterizes those glyphs into cached GPU
masks and submits them. The single render-world entry to a `Buffer` is
`layer_pool_text_render::build_text_buffer`, which builds a `TextLayoutRequest`
from the retained run record + paint op and calls the seam's
`text_layout::text_render::build_render_text_buffer` — no render path
constructs a `cosmic_text::Buffer` directly.

## The buffer LRU and glyph-mask cache

- `TextRunRenderState::text_buffer_cache` is an
  `LruCache<TextBufferKey, CachedTextBuffer, 128>` keyed by the density-free
  text fields (`text_buffer_key`). A `CachedTextBuffer` holds the shaped
  `Buffer` so a repaint of an unchanged run does not re-shape.
- `TextRunRenderState::glyph_mask_cache` is an
  `LruCache<GlyphMaskKey, CachedGlyphMask, 1024>` keyed by swash's `CacheKey`
  (density-specific). `build_glyph_mask` rasterizes through
  `swash_cache.get_image` into an R8 texture and bind group.

## Font registration and warmup

- FontSystem #2 loads faces through `register_font_asset`, which feeds
  extracted asset bytes into `font_system.db_mut()`. `FontRegistrationState`
  declares each FACE's required asset ids — keyed by `FontFaceId` (the catalog
  face id, text seam S3), not the removed `TextFontFamilyId` placeholder — and
  reports a face ready only when every required id is registered. A run's face
  id is resolved from its family list (`text_layout::text_render::resolve_face_id`)
  before the readiness gate.
- Consumer-installed faces reach FontSystem #2 through the extract pipeline
  (`extract_text_font_assets`): the shared `TextFontCatalog` enumerates the
  consumer faces, each is minted one `UiDomFontAsset` (deduped by
  `FontFaceId` in `UiDomTextFontExtractState`), and its bytes are queued for
  `register_font_asset` exactly once.
- `PreloadWarmupQueue` warms glyph masks before first paint: requests are
  deduped by `PreloadKey` and drain in FIFO batches (`UI_TEXT_PRELOAD_BATCH_SIZE`,
  8) across five language hints × ten warmup samples × ten font sizes.

## FontSystem #2

`create_text_run_font_system` is the render-owned `FontSystem` (FontSystem #2),
distinct from the main-world thread-local one on `TextLayoutContext`. Each
owns a private fontdb by cosmic-text's design; the shared SOURCE list is the
catalog, not the FontSystem.

## The debug-rect demotion

When a glyph's id is 0 (`glyph_requires_debug_fallback`) — the honest render
answer for a face that has no glyph for the requested script — the whole run
is demoted to a debug rect: `prepare_text_buffer_glyphs` aborts, the run is
pushed to `fallback_debug_runs`, and `draw_text_run_fallback_debug_rects`
draws one `GpuDebugRectOp` using the run's position/size/color. An unsupported
glyph-mask result demotes the same way.

## Invariants, rules, and decisions

- `LayerPool` is the only retained backing-image allocator in `ui-dom`; retained
  work is addressed by `LayerLease`, never a raw `Handle<Image>`.
- No CPU rasterization fallback, no mirrored per-layer CPU render payload, and
  no per-op allocation in the hot path; translation happens only at the final
  GPU submission boundary (see `mod.rs`'s hard boundary rules).
- The render text path reaches a `Buffer` only through the seam's public render
  entry; it never constructs a `cosmic_text::Buffer` directly.
- The retained run bank (`TextRunRenderState::run_bank`) is the render world's
  copy of the canonical text-run registry (`LayerPool::text_runs`); a run whose
  record or font family is not ready defers its surface via
  `PendingLeaseDependencies` instead of drawing partially.
