# PLAN — Render-Ops Staging Extraction (`RenderOpsHandler`)

Status: target-state architecture spec. No code has been changed under this plan yet.
Scope: the **main-world handoff** of paint ops, inside `crates/ui-dom`. Everything
outside that handoff is preserved.

---

## 1. Problem Statement

Paint ops are produced deep inside `RootDom::update` and cross into render-world state
immediately, invisibly, and by copy:

- **Unobservable.** A `LayerPaintPlan` is built per dirty layer inside `paint_layer`
  and handed straight into the app-global, crate-private `LayerPoolRenderJobQueue`.
  No public accessor exists at any point in the ops' lifetime. Tests cannot see the
  exact ops an update produced without adding latency, allocations, or new concepts.
- **Copied.** The commit facade takes `&LayerPaintPlan` and clones it
  (`plan.clone()`, `domains/layer_pool/layer_pool.rs:595`); the original is dropped.
  The copy exists for signature reasons only.
- **Deep.** Because the boundary is invisible, it has accreted blind "fixes" over
  time — revision-shaped state with no consumers, an unconditional clear — that a
  shallow, observable boundary would not invite (§2.7).

**End state of this work:** the exact render ops produced by a `RootDom::update` are
observable and assertable in the main world, between the update that produced them and
the once-per-tick move that hands them to the Bevy render world.

The largest beneficiary is `tests/layout_proofs/`. Those tests previously could not
see where elements actually render and compensated with parallel re-implementations of
the offset math — e.g. `tests/layout_proofs/support.rs:334-401`
(`accumulated_border_box_edge_px`) re-walks DOM ancestors and sums `border_box` edges
to reconstruct positions the paint stage already knows. After this work, the proofs
assert against the staged ops themselves (layer-local pixel geometry), roughly 90% of
them can be drastically simplified, and proofs for simple currently-off-spec CSS cases
become writable.

A second deliberate beneficiary: the staging object is a **sealed, standalone thing
that is not a `RootDom`**. Anything that produces `LayerPaintPlan`s — animation test
harnesses, custom renderers, future non-DOM producers — stages ops through the same
owner and the same move path.

## 2. Background

### 2.1 Current architecture (brief)

`ui-dom` is a retained DOM with browser-style derived pipelines. `RootDom` owns the
authored tree and per-node caches; `RootDom::update` runs the named stages —
lifecycle reconciliation, style, layout, paint, composite-order publication — inside
`domains/composite`. Bevy integration exists at exactly two couplings:

### 2.2 Coupling #1 — the layers (preserved; not changed by this plan)

Each live layer is represented in three data structures kept consistent by contract:
`RootDom`'s `layer_entity_by_root` / `layer_lease_by_root` maps; the Bevy entity
(`BevyLayer` + `UiDomLayerPresentation` components + ECS hierarchy); and the
app-global `LayerPool` record (`Handle<Image>` + mapping + policy), addressed by
generation-validated `LayerLease` tokens. This coupling buys Bevy visibility culling,
ECS parent/child propagation, `RenderLayers` inheritance, and per-root independence
for a thousand `RootDom`s. The operator confirmed: "we LIKE the benefits that we gain
from using the Bevy entities as layers." **It is explicitly preserved.**

### 2.3 Coupling #2 — the render ops (the one thing this plan changes)

Current lifecycle of one layer's ops:

1. `paint_layer` builds a fresh `LayerPaintPlan` per dirty layer and calls
   `LayerPool::apply_paint_plan_in_world(world, lease, &plan)`.
2. That facade validates the lease, applies policy, acquires the surface, then
   **clones the plan** into `PendingLayerApply` (layer_pool.rs:595) and enqueues it in
   the app-global `LayerPoolRenderJobQueue`.
3. Bevy's extract drains the queue once per tick into `LayerPoolRenderExecutor`.
4. The executor resolves `GpuImage`s and runs per-family wgpu passes
   (rects → icons → text runs → svgs → images).

Side effects of the current shape: paint dirt is cleared *during plan building*
(`domains/paint/mod.rs:253-257`), before commit — a failed or stalled commit loses the
dirty state; and ops from all roots interleave in one unattributed queue.

**Scope note — per-root vs app-global (as implemented).** `RootDom` state (DOM,
dirty flags, layer maps, pending queues, events, metrics, scroll offsets) and the
Bevy layer entity subtrees are per-root. `LayerPool` (surfaces, free pool, text-run
interning, metrics), `LayerPoolRenderJobQueue`, and `LayerPoolRenderExecutor`
(pending applies, GPU pipelines, glyph/text-buffer caches, icon atlas, surface-order
maps) are app-global. Fonts exist twice: main-world measurement uses a
`thread_local!` cosmic-text context over `include_bytes!` embedded fonts; the render
world builds its own `FontSystem` from Bevy-loaded `UiDomFontAsset` bytes. A thousand
roots update independently today; their ops funnel into one shared queue with no root
attribution.

**The extract drain is already a single per-tick operation — but it is global**, not
per-root, and it moves the *clone*, not the ops the paint stage built.

**Root attribution across the render boundary was considered and rejected.** The
render world consumes `(lease, image handle, layer-zero offset, ops)` and nothing
else; a root tag would be dead bytes carried across the boundary and dropped.
Attribution exists for free in the main world — each plan is produced inside one
root's `update`, addressed by a lease registered in exactly one root's
`layer_lease_by_root` — and it dies at the move. This plan keeps attribution entirely
main-world-side, at context granularity (§4.2).

### 2.4 Operator constraints discovered during review

The operator clarified what is liked in the current system and what the ideal shape
is. These are constraints this plan honors:

- **LIKE: Bevy entities as layers** (§2.2) and their benefits — preserved.
- **LIKE: a single buffer reused between renders that holds all the ops.**
  Fact-checked against the implementation: the structures that genuinely have this
  property today are the job queue's persistent `jobs: Vec` (drain-preserved
  capacity) and the per-pipeline grow-only GPU instance buffers. The per-layer plan
  `Vec`s and the executor's `pending_applies` (`std::mem::take`, capacity **not**
  reused) do not. The handler is designed to extend the liked property: retained
  `Vec`s, drained with capacity preserved (§4.1).
- **LIKE: named stages at the top level of a DOM UI update** (lifecycle
  reconciliation, style, layout, paint, composite publication) — preserved; this plan
  changes nothing about stage structure.
- **LIKE: a thousand `RootDom`s operating independently at the same time in Bevy** —
  preserved; per-root state stays root-owned, and handler sharing granularity
  (context) is the app's choice.
- **IDEAL WORLD, in the operator's words (paraphrased):** the moving of render ops
  from Bevy's Main World to the Render World should be a single operation that
  happens once per Bevy tick, after that particular `RootDom` has finished its other
  stages — so the exact render ops that particular update generated from particular
  input are visible. The handoff should ideally be a *move*, not a copy.

### 2.5 Design decision: Option A vs Option B

Two shapes were evaluated:

- **Option A** — a `render` call on `RootDom`, called after `update`; each root owns
  its staging buffer.
- **Option B** — a brand-new thing whose only job is the render ops, set on
  `RootDomContext` similarly to the font/text caches; all `RootDom`s write ops to it;
  when Bevy extracts, it performs the move for everything staged.

**Option B was chosen.** Operator rationale: the crate has reached a level of
complexity where complete extraction from `RootDom` gives a good testable seam; the
handler can exist as a sealed thing the app uses to test animations or custom
rendering; and other things that are not a `RootDom`, with entirely different flows,
can use the same render-ops handler. Option A structurally cannot serve non-DOM
producers. Consequence: the earlier idea of a separate main-world `render()` call
disappears — extract performs the move; the observability window is between `update`
and extract; hosts and tests without a render app use `take_staged()` as the manual
move.

### 2.6 Agreed recommendations (from the review Q&A)

1. One staging boundary (the handler); it is always accepting; extract takes whatever
   is present; no host-visible render call.
2. The staged unit is today's apply payload (lease + image handle + layer-zero +
   owned plan); the render world needs no re-split.
3. The handler is sealed and dumb: no validation, no policy beyond queue policy;
   validation stays in the `LayerPool` facade path; external producers hold real
   leases.
4. Text-run interning stays in `LayerPool` this session; text-content observability
   is out of scope.
5. Dirt clears exactly at the terminal commit decision (C3, §4.4).
6. Deletions: only what the redesign itself makes dead.
7. Remaining vagueness is deferred to implementation time (§9), to be resolved within
   this scope, intent, and the performance expectations.

### 2.7 Confirmed debris (context — not targets — not preserved)

**NOTE 1 (operator):** previous agents had a strong bias to inject CLEAR operations
randomly throughout the render pipeline. If the design is correct, no clears are ever
needed: any draw is an overlapping write on the previous *retained* content. Found
instance, confirmed in code: the rect pass clears the whole surface to transparent on
every apply — `LoadOp::Clear(TRANSPARENT)` at `domains/layer_pool/mod.rs:793` — and
runs even when `rects` is empty. That is one full-surface write per layer per apply:
likely incorrect and a major performance hit. **Not this plan's target**; recorded so
a future change can make clear-semantics an explicit paint-stage decision (or prove
coverage and drop the clear).

**NOTE 2 (operator):** previous agents had a strong bias to inject "revisions" or
"versions" throughout the data structures for no benefit, then consume those values
with semi-random `if` statements, causing critical bugs — including attaching time
values to revisions via OS calls. The same agents then refused to remove them without
secretly adding new revision-like behavior elsewhere. Guidance carried forward: in
this crate the most we ever need is *dirty* behavior to opt into or out of work. If
revision-like values appear, bugs or other fuckery are extremely likely nearby (that
code was likely written in a low-coherence state) — question everything around them.

Prior-work anomaly inventory, with dispositions (no preservation effort is owed for
these, unlike the rest of the code; they are **not** targets of this plan):

| Artifact | Evidence | Disposition |
|---|---|---|
| Unconditional full-surface clear on every apply (rect pass, `LoadOp::Clear(TRANSPARENT)`, even when `rects` is empty) | `domains/layer_pool/mod.rs:793` | NOTE 1 instance. **Not changed by this plan** (§2.7 NOTE 1). |
| Dead paint-revision channel: `LayerRevisionState`, `next_paint_revision()`, `mark_layer_full_repaint()`, `layer_revisions_by_root` | `types/root_dom.rs:172-175, 1339-1350`; zero production callers | Revision-shaped state with no consumer. Not a target; may be deleted opportunistically at implementation time since `root_dom.rs` is touched anyway (§9). |
| Write-only damage-rect machinery: `DomPaintState::DamagedRectsRepaint(ArrayVec<DomRect, 8>)` etc. | `types/dom_node.rs:113`, `domains/paint/mark_dirty.rs`; execution reads only `is_dirty()` | Not a target. Deletion verdict at implementation time (§9). |
| `generation` fields on intrinsic records | `types/dom_node.rs:164-203` | Flagged as revision-shaped; consumers not yet convicted. Verdict at implementation time (§9). |
| **Kept as legitimate:** `StyleRuntime.revision` / `style_runtime_revision` (dirty-broadcast), `LayerLease.generation` (ABA protection), `RootDom.clock_s` (host-authored transition time; no OS clock calls exist in this crate — verified) | — | Preserved. |

**Carried-forward rule:** the new staging path adds no sequence numbers, no
revisions, no time fields, and no clear semantics. The staged buffer is data, moved.

**Method note:** an earlier architecture pass described this crate as *intended*
rather than as *implemented* and in doing so omitted the `plan.clone()` at the commit
boundary. Every boundary claim in this plan was re-verified line-by-line against the
implementation. The same standard applies to implementing this plan: verify, don't
intuit.

## 3. Proposal Overview

Insert exactly **one new owner** between "paint produces a plan" and "the render
world receives work": a sealed, main-world staging object — `RenderOpsHandler` —
owned by `RootDomContext`, that holds the tick's outgoing ops until Bevy's extract
moves them once per tick.

```
BEFORE:  paint_layer ──build plan──▶ apply_paint_plan_in_world ──CLONE──▶ app-global
                                                                        job queue
AFTER:   paint_layer ──build plan──▶ acquire_paint_target ──MOVE──▶ RenderOpsHandler
                                          (LayerPool)              (context-owned,
                                                                    observable)
```

Everything else is identical: same plans, same payloads, same extract cadence, same
executor, same wgpu passes, same readiness stalls, same queue policy semantics
(upsert-by-lease, best-effort deferral, invalidate-clears-conflicting).

**Change surface, stated plainly:** 4 new files (the `domains/render_ops/` module),
10 existing files modified (§5), and the remaining 69 of the crate's 79 source files
untouched — including **every render-world execution file, every domain algorithm
(layout, style, visitors, text layout), and every shader.**

What this buys:

- **Observability (the point):** between `update` and extract, the exact staged ops
  are readable through the context's handler — zero-alloc read access, or take
  ownership manually without any render app.
- **Move, not copy:** the plan is moved into the handler; the clone at
  layer_pool.rs:595 is deleted by the signature change.
- **A sealed, reusable seam:** the handler is not a `RootDom`; external producers
  (animation/custom-render tests, future non-DOM producers) can hold one and stage
  through the identical path.
- **Dirt-clearing becomes honest:** paint dirt clears exactly when the paint pass
  reaches a terminal decision (staged / suppressed / empty), never on error (C3, §4.4).

## 4. Proposed Architecture

### 4.1 New module: `domains/render_ops/`

A sibling of `layer_pool`, wired in `domains/mod.rs`, re-exported through
`domains::*`. Per structural rule 1, each behavior type in its own file:

- `render_ops_handler.rs` — `RenderOpsHandler` (public).
- `staged_layer_apply.rs` — `StagedLayerApply` (public).
- `staged_external_rgba_upload.rs` — `StagedExternalRgbaUpload` (public).
- `mod.rs` — module doc, wiring, re-exports.

```rust
/// One staged layer repaint: owned ops plus everything the render world
/// needs to draw them into the right surface. Moves end-to-end; never cloned.
pub struct StagedLayerApply {
    pub lease: LayerLease,
    pub image_handle: Handle<Image>,
    pub layer_zero_in_image_px: [f32; 2],
    pub plan: LayerPaintPlan,
}

pub struct StagedExternalRgbaUpload {
    pub lease: LayerLease,
    pub image_handle: Handle<Image>,
    pub image_extent_px: UVec2,
    pub rgba_bytes: Vec<u8>,
}
```

`RenderOpsHandler` is `Arc`-shared, internally synchronized (same pattern as
`RootDomContext::style_runtime: Mutex<StyleRuntime>`), and **sealed**: no validation,
no `World`, no Bevy, no policy beyond queue policy, no metrics beyond (open) its own
named counters. Its state is retained staging storage — staged applies, staged
external uploads, and deferred best-effort applies — held in persistent `Vec`s
drained by `drain(..)` so capacity persists across frames.

```rust
impl RenderOpsHandler {
    pub fn new() -> Self;

    /// Accepts one owned apply; applies queue policy (upsert-by-lease;
    /// BestEffort defers past the queue limit). Infallible.
    pub fn stage(&self, apply: StagedLayerApply, policy: LayerPoolPolicy) -> StageOutcome;

    /// Accepts one owned external RGBA upload (upsert-by-lease).
    pub fn stage_external_rgba(&self, upload: StagedExternalRgbaUpload);

    /// Removes staged applies/uploads (incl. deferred) for one lease (§4.3.3).
    pub fn invalidate_lease(&self, lease: LayerLease);

    /// Zero-allocation read access to retained staged applies, in stage order
    /// (= paint order = DOM layer order per root).
    pub fn with_staged_applies<R>(&self, f: impl FnOnce(&[StagedLayerApply]) -> R) -> R;

    /// Moves everything currently staged out to the caller — the manual move
    /// for hosts/tests without a render app.
    pub fn take_staged(&self) -> RenderOpsBatch;
}
```

(`StageOutcome` = `Staged | Deferred`, used to keep the existing `UploadDeferred`
metric on the existing `LayerPool` tally. `RenderOpsBatch` is the owned payload moved
out of the handler at drain: the staged applies and staged external uploads present
at drain time — deferred work is promoted only *after* a drain, so it is never part
of the batch that just left (§4.3.2). Exact accessor set beyond the minimum is
open — §9.)

### 4.2 Ownership and registration

- `RootDomContext` gains `render_ops: Arc<RenderOpsHandler>`; default construction
  creates a fresh handler, and a second constructor accepts a caller-provided
  `Arc<RenderOpsHandler>` so contexts can deliberately share one handler (and external
  producers can hold one with no `RootDom` at all).
  - Attribution granularity is therefore **context**, not root — and sharing is the
    app's choice. Per-root observability for a given root = give it its own context.
    Nothing about attribution crosses the render boundary (§2.3).
- A small main-world registry resource, `RenderOpsHandlers(Vec<Arc<RenderOpsHandler>>)`
  (installed by `UiDomRenderPlugin`; lazily initialized at registration time when the
  plugin is absent — the same pattern `LayerPoolRenderJobQueue` uses today), is how
  the render world's extract finds handlers. Dedup by `Arc::ptr_eq`; drain order =
  registration order (deterministic).
- **Registration contract:** `RootDom::attach_to_bevy_parent_entity` registers its
  context's handler (idempotent) — attach precedes any possible paint, so no paint can
  escape registration. External producers register explicitly via
  `register_render_ops_handler(&mut World, &Arc<RenderOpsHandler>)`. Detach leaves
  registration in place: a drained handler is inert.
- `RootDomContext::render_ops()` (public) returns the `Arc<RenderOpsHandler>`.

### 4.3 Flows

#### 4.3.1 RootDom paint path (target)

Inside `paint_layer` (`domains/paint/mod.rs`) — the only place ops are born. The
steps below are a **decision flow**, not a straight-line sequence: exits marked
**terminal** trigger the single clearing act (C3); exits marked **not terminal**
leave paint dirt untouched.

1. Early-outs preserved, **not terminal** (dirt untouched, matching today):
   `PaintBackend != Gpu`; no layer entity.
2. `plan = build_layer_paint_plan(root_dom, &mut LayerPool, …)` — owned; the
   `&mut LayerPool` borrow still serves text-run interning, unchanged (§6).
3. `plan.is_empty()` → **terminal, stage nothing** (preserves today's empty-plan
   early-out exactly: no validation, no suppression metric, no surface touch).
4. `target = LayerPool::acquire_paint_target_in_world(world, lease)?` — the shrunken
   writer seam, in today's exact order: validate lease (`StaleLeaseRejected`) →
   policy check (`Offscreen`/`External` → `UploadSuppressed` metric) → require
   direct GPU executor → acquire surface. Outcomes:
   - `Ok(Some({ image_handle, layer_zero_in_image_px, policy }))` → proceed to 5.
   - `Ok(None)` (suppressed by policy) → **terminal, stage nothing**.
   - `Err` (`InvalidLease` / `MissingDirectGpuExecutor`) → **not terminal**
     (C3: dirt untouched — today's clear-before-failed-commit hazard removed).
5. `handler.stage(StagedLayerApply { lease, ..target, plan }, policy)` — **MOVE** →
   **terminal**. `StageOutcome::Deferred` → `LayerPool` metric `UploadDeferred`
   (same tally, same name, same count as today).

Every **terminal** exit (3, 4-suppressed, 5) performs the same single clearing act
for the layer subtree's paint dirt (C3); no clearing happens during plan building or
on **not terminal** exits. `RootDom` metric `LayerPainted` increments at the terminal
exit iff the plan was non-empty — today's exact condition, so staged and suppressed
both count, empty does not.

`LayerPool::apply_paint_plan_in_world` is removed. Its responsibilities split exactly
along the existing seam: validation/suppression/acquisition stay on `LayerPool`
(step 4); queueing moves to the handler (step 5). In-workspace callers are only
`paint_layer` and `ui-dom`'s own tests (verified).

#### 4.3.2 Extract path (target — once per tick, render world)

`extract_layer_pool_render_work` keeps its current steps (feedback flush, font
assets, icon atlas poll, text-run drain, `LayerPool` command-queue drain) and gains
one final step:

```
for each handler in RenderOpsHandlers (registration order):
    lock;
    drain staged applies + staged external uploads
        → feed each to the executor directly (no job-enum wrapper;
          apply-by-lease replace semantics preserved);
    promote deferred best-effort applies into the handler's staged storage
        (promote-after-drain, exactly as today: promoted work executes
        no earlier than the next tick, never in the batch that just left);
    unlock;
```

This is **the move**: staged `Vec` content is drained (capacity retained) into
executor ownership. No op is copied on this path. Executor behavior — readiness
stalls, re-queues, per-family passes, ordered-surface publication — is untouched.

#### 4.3.3 Invalidation broadcast (correctness-critical)

A staged apply must never execute against a destroyed or repurposed lease: a released
surface handle may be reused by another layer of the same extent, so an orphaned
staged apply would paint into the wrong surface. Today this is prevented because
invalidation and applies share one queue.

Under this plan, applies live in handlers, so lease invalidation is broadcast:
`enqueue_layer_invalidation(world, lease)` keeps pushing `Invalidate` into the
`LayerPool`-owned command queue (the executor's stalled pending applies still need
it) **and** iterates `RenderOpsHandlers` calling `handler.invalidate_lease(lease)`.
The `External`-policy bypass (`InvalidationBypassed` metric) is preserved unchanged.
The remaining command queue keeps its three command variants (`Invalidate`,
`ReplaceRootSurfaceOrder`, `DetachRootSurfaceOrder`).

### 4.4 Contracts

- **C1 — Move-only ops.** A `LayerPaintPlan` is owned by exactly one place at any
  moment: builder → staged item → executor → dropped after `write_buffer`. No clone
  exists anywhere in the paint→executor path.
- **C2 — Stage acceptance is infallible and terminal.** The handler never rejects,
  never reorders (beyond upsert-by-lease), never inspects op content.
- **C3 — Clear-on-terminal-decision.** A layer subtree's paint dirt is cleared exactly
  when its paint pass reaches a terminal decision: staged, suppressed, or empty plan —
  and **not** when the writer seam errors (`InvalidLease` / `MissingDirectGpuExecutor`)
  or on the step-1 early-outs. This replaces today's clear-during-build hazard (dirt
  lost on failed commit). The cleared node set is exactly the set today's paint walk
  clears — the painted subtree **plus** the select-popup skipped subtrees
  (`clear_subtree_paint_repaint_state`) — now cleared in one act at the terminal
  point instead of per node during the walk; text-run hint write-back during the walk
  is unaffected.
- **C4 — Steady state.** No edits → no staged ops → extract moves nothing → existing
  metric silence holds. Buffers persist; nothing allocates.
- **C5 — Deterministic order.** Stage order within a root = paint order = DOM layer
  order. Across roots sharing one handler = call order of their updates. Across
  handlers at extract = registration order. Tests may rely on index.
- **C6 — Sealed handler.** No validation, no `World`, no Bevy, no revisions, no time
  fields, no clear semantics, no per-op allocation.
- **C7 — Observability without side effects.** Reading staged ops never mutates the
  handler, never allocates, never touches the render world.
- **C8 — Invalidation reaches staged work** (§4.3.3). A lease destroyed between
  staging and extract cannot execute.

### 4.5 Observability and test correlation

Tests identify staged applies by (a) index (C5) — sufficient for the common
single-layer proof — or (b) `LayerLease` (see §9: making `RootDom::layer_lease(key)`
public; it is an opaque `Copy` capability already exposed publicly inside
`ExternalLayerTarget.lease`, and mutation requires the `World` facades, so exposure
grants no authority).

Text ops assert on geometry and style fields (`position_px`, `size_px`,
`font_size_px`, `line_height_px`, `wrap_mode`, `alignment`, `color_rgba`).
`TextRunId` remains `pub(crate)`; content resolution stays in `LayerPool` (§6).

## 5. Detailed Product Changes

### 5.1 New files (4)

| File | Contents |
|---|---|
| `domains/render_ops/mod.rs` | Module doc, wiring, re-exports |
| `domains/render_ops/render_ops_handler.rs` | `RenderOpsHandler`, `StageOutcome`, staging + queue policy |
| `domains/render_ops/staged_layer_apply.rs` | `StagedLayerApply` |
| `domains/render_ops/staged_external_rgba_upload.rs` | `StagedExternalRgbaUpload` |

### 5.2 Modified files (10)

| File | Change |
|---|---|
| `domains/mod.rs` | Wire `render_ops` module; export new public types |
| `domains/paint/mod.rs` | `paint_layer`: stage into the context handler (§4.3.1); remove `commit_gpu_paint`; clear-on-terminal-decision (C3) |
| `types/root_dom_context.rs` | Add `render_ops: Arc<RenderOpsHandler>`, constructor variant, `render_ops()` accessor |
| `types/root_dom.rs` | `attach_to_bevy_parent_entity` registers the handler (idempotent) |
| `render_plugin.rs` | Install the `RenderOpsHandlers` registry resource |
| `domains/layer_pool/layer_pool.rs` | Remove `apply_paint_plan*`; add the `acquire_paint_target*` writer seam (validation, suppression, surface acquisition — unchanged logic) |
| `domains/layer_pool/mod.rs` | Extract drains registered handlers (§4.3.2); invalidation broadcast (§4.3.3); staged payload types move to `render_ops` |
| `domains/layer_pool/layer_pool_render_job_queue.rs` | Stops receiving `Apply` / `ExternalRgbaUpload`; keeps the three command variants; deferred-best-effort storage and promotion move to the handler |
| `domains/layer_pool/layer_pool_render_job.rs` | Variants shrink accordingly |
| `domains/layer_pool/layer_pool_render_executor.rs` | Accepts the `render_ops` staged payload types (renames only; behavior identical) |

Crate `README.md` sections referencing the old apply path are updated in the same
change (repo rule). `tests/perf_metric_contract.rs` pin updates, where required, land
in their own commit with justification (repo rule: pin movement = performance change).

**Explicitly untouched:** `domains/composite/*`, `domains/layout/*`, `domains/style/*`,
`domains/visitor/*`, `domains/text_layout/*`, `domains/layer_ops/*`,
`domains/hit_test.rs`, `domains/layer_hit.rs`, `domains/icon_atlas.rs`,
`domains/control_widgets.rs`, `domains/keyboard.rs`, `bevy/*` (all five files +
shader), `types/dom_node.rs`, `types/root_dom_fragment.rs`, all
`domains/layer_pool/` render-side files (`layer_pool_render_state.rs`,
`layer_pool_text_render.rs`, `layer_config.rs`, `layer_lease.rs`,
`layer_pool_policy.rs`, `layer_pool_metric*`, `external_layer_target.rs`,
`layer_pool_render_feedback.rs`) and all six paint-family shaders.

### 5.3 Public API deltas

Added:
- `RenderOpsHandler`, `StagedLayerApply`, `StagedExternalRgbaUpload`, `StageOutcome`
  (via `ui_dom::domains::*`).
- `RootDomContext::render_ops() -> Arc<RenderOpsHandler>`.
- Context constructor accepting a shared `Arc<RenderOpsHandler>` (exact form open, §9).
- `register_render_ops_handler(&mut World, &Arc<RenderOpsHandler>)`.

Removed / replaced:
- `LayerPool::apply_paint_plan_in_world(world, lease, &plan)` → replaced by the
  writer seam `LayerPool::acquire_paint_target_in_world(world, lease)` +
  `RenderOpsHandler::stage(...)`. (Writer-seam visibility open, §9; `pub(crate)`
  suffices for `paint_layer`.)
- `LayerPool::submit_external_rgba_in_world(world, target, bytes)` → gains a handler
  destination (handler parameter on the facade, or split with validation on
  `LayerPool` and staging on the handler; exact shape open, §9). Validation semantics
  (generation / policy / handle / extent) preserved unchanged.

### 5.4 Deletions (only what the redesign itself makes dead)

- The `plan.clone()` at `layer_pool.rs:595`.
- `LayerPool::apply_paint_plan*` (replaced, §5.3).
- `PendingLayerApply` / `PendingExternalRgbaUpload` as separate types — superseded by
  the public staged payload types.
- The queue's `Apply` / `ExternalRgbaUpload` variants — queue policy re-homed to the
  handler.

No other deletions are part of this plan. (Debris from §2.7 is not preserved if the
work naturally encounters it, but this plan does not go looking for it.)

## 6. Out of Scope

- **Coupling #1 (layers)** — preserved in full (§2.2).
- **The render world** — executor, readiness stalls, re-queues, wgpu passes, ordered
  surface render pass, shaders: untouched.
- **Text-run interning and text content observability** — `LayerPool`'s canonical run
  bank and extraction stream stay exactly as they are; text ops are asserted on
  geometry/style fields only. This is the known, accepted second main→render side
  channel.
- **The unconditional rect-pass clear** (§2.7) — recorded, not changed.
- **Damage-rect machinery, dead paint revisions, intrinsic `generation` fields** —
  not targets (§2.7); verdicts at implementation time where the work touches them (§9).
- **The `layout_proofs` simplification itself** — this plan delivers the seam that
  makes it possible; the proof rewrite is follow-up work.
- **Metric-contract pin updates** — expected where the op flow changed; handled in
  their own commit per repo policy.

## 7. Acceptance Story — the gate

### 7.1 Before: the compensating hack

Today, a layout proof that wants to assert "this element renders at (x, y)" cannot
see the paint output. `tests/layout_proofs/support.rs:334-401`
(`accumulated_border_box_edge_px`) instead re-walks the element's DOM ancestors and
accumulates `border_box` edges — a second, parallel implementation of the offset math
that lives inside the layout/composite code it is meant to verify. The two can drift:
the helper proves what *its own* math produces, not what the paint stage emitted.
This hack is also why proofs for several simple CSS cases cannot be written today:
those cases are off-spec, and only exact op output can pin the difference.

### 7.2 After: asserting the ops directly

```rust
let (mut world, mut root) = attached_gpu_root_dom_setup(320.0, 320.0);
// ...build a minimal document, then:
root.update(&mut world, 0.0, DomPointerUpdate::Unchanged)?;

let ops = root.context().render_ops();
ops.with_staged_applies(|applies| {
    let plan = &applies[0].plan;              // document root layer, first paint
    assert_eq!(plan.rects[0].position_px, [16.0, 24.0]);
    assert_eq!(plan.rects[0].size_px, [120.0, 40.0]);
});
```

The read happens after `update`, before extract. In tests without a render app,
extract never runs, so staged ops persist until `take_staged()` moves them out.
Single-layer proofs (the common case): layer-local op geometry **is** the screen
position. Multi-layer proofs assert per staged apply; order is deterministic (C5).

### 7.3 Gate scenarios

All of the following must pass before this work is accepted. Each scenario names what
it proves.

- **G1 — exact rect geometry, single layer.** Minimal document, one update; assert
  `plan.rects[i].position_px` / `size_px` / colors exactly. *Proves: the
  `accumulated_border_box_edge_px` class of helper is replaceable.*
- **G2 — op order within a layer is DOM order.** Multiple children; rect ops appear
  in child order. *Proves: ordering contract C5 within a plan.*
- **G3 — multi-layer document.** One staged apply per painted layer root, in
  deterministic order; correlatable by index (and by lease if §9.4 opens). *Proves:
  per-layer staging granularity — no merged op soup.*
- **G4 — text ops.** Assert `position_px`, `size_px`, `font_size_px`,
  `line_height_px`, `wrap_mode`, `alignment`, `color_rgba`. Content/glyph assertions
  are out of scope (§6). *Proves: text geometry is observable without touching the
  text-run side channel.*
- **G5 — empty plan stages nothing.** A zero-size / fully unpainted layer produces no
  staged apply. *Proves: today's empty-plan early-out is preserved.*
- **G6 — steady state is silent.** A second `update` with no edits stages zero ops.
  *Proves: C4 — the cheap steady-state rule extends across the new seam.*
- **G7 — handler sharing granularity.** Two roots sharing one handler: both roots'
  ops arrive in stage order. Two roots with separate handlers (the default): full
  isolation. *Proves: context-granularity attribution, with zero attribution bytes
  crossing the render boundary.*
- **G8 — detach eviction.** Stage ops, then detach (or destroy the lease) before
  extract: that lease's staged applies are evicted (C8). *Proves: a staged apply can
  never execute against a destroyed or reused surface.*
- **G9 — external producer, no RootDom.** Create a lease
  (`LayerPool::create_layer_lease_in_world`), acquire its surface, build a plan,
  stage it into a standalone handler, register the handler, and observe extract move
  it. *Proves: the sealed seam serves non-DOM producers (animation / custom-render
  test harnesses, future different-flow users of the handler).*
- **G10 — executor-visible behavior unchanged.** Existing executor, layer-pool, and
  ordered-surface tests pass unmodified; applies arrive with identical payloads,
  order, readiness-stall and re-queue semantics. *Proves: the render world was not
  changed by a main-world refactor.*

### 7.4 The simplification gate

The work is accepted when:

1. Every gate scenario G1–G10 passes.
2. The offset-accumulation helpers in `tests/layout_proofs/support.rs` are deletable
   (their deletion is follow-up work, §6 — but nothing in this plan blocks it).
3. Roughly 90% of the existing proofs can express their assertions directly against
   staged ops instead of the parallel offset math — demonstrated on a representative
   sample before the rewrite begins.
4. At least one new proof for a currently off-spec simple CSS case can be written
   against exact staged ops.

Mapping to §8: criteria 1–2 ⇐ G1–G2 (plus the C1 grep); criterion 3 ⇐ G7;
criterion 4 ⇐ G8; criterion 5 ⇐ G6; criterion 6 ⇐ G10; criterion 7 is the repo gate.

## 8. Acceptance Criteria

1. No `plan.clone()` (or any plan clone) exists in the paint→executor path.
2. A test can build a root, run one `update` with known input, and assert the exact
   staged rect ops (positions/sizes/colors) from `context().render_ops()` — without a
   render app installed.
3. Multiple roots sharing one handler: all ops arrive at extract in stage order;
   roots with separate handlers: full isolation; extract drain order is deterministic.
4. Detach (or lease destruction) between `update` and extract evicts that lease's
   staged applies (C8) — pinned by a test.
5. Steady state: repeated updates with no edits stage nothing; metric silence holds.
6. Executor-visible behavior is unchanged: same applies, same order, same payloads,
   same readiness-stall and re-queue semantics.
7. `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`,
   `cargo test --workspace`, `cargo xtask lint` pass; `perf_metric_contract.rs` pin
   updates, where required, land in their own commit with justification.

## 9. Open Questions

(decide at implementation, within scope, intent, and performance expectations)

1. Exact handler accessor set beyond the minimum (§4.1).
2. Visibility of the writer seam `acquire_paint_target_in_world` (§5.3).
3. Final shape of the external-RGBA staging API (§5.3).
4. Whether `RootDom::layer_lease(key)` becomes public (§4.5).
5. Opportunistic deletion verdicts for the §2.7 debris the work touches: dead
   paint-revision channel (same file as a planned change), `DamagedRectsRepaint`
   machinery, intrinsic `generation` fields.
6. Handler metric names, if any (zero-alloc emission; steady-state silence;
   pin updates in their own commit).
7. Exact form of the context constructor accepting a shared handler (§4.2).

## 10. Implementation record (landed on branch `render-ops-handler`)

Three commits:

- `fa650d6` — `feat(ui-dom): add sealed render-ops staging module` (additive:
  `domains/render_ops` + op-vocabulary re-exports; no behavior change).
- `d23bc8e` — `feat(ui-dom)!: move render ops to context-owned RenderOpsHandler
  staging` (the intentional behavior change: C1 move-only ops, C3
  clear-on-terminal-decision, writer seam, extract drain, invalidation
  broadcast, registration, API deltas, gate tests G1–G9).
- `0f7a807` — `test(ui-dom): re-pin perf contracts for render-ops staging`
  (own commit per policy; every moved pin justified inline).

Deltas from this plan as written (all within scope and intent):

- §5.1: the new module landed as **5 files, not 4** — `render_ops_batch.rs` was
  split out per structural rule 1. The `RenderOpsHandlers` registry and
  `register_render_ops_handler` live in `render_ops/mod.rs` (derive-only type +
  free fn; lint-clean).
- §5.2: `layer_pool_render_state.rs` also changed — one mechanical type rename
  (`PendingLayerApply` → `StagedLayerApply` in `apply_plan_to_surface`'s
  signature). It was listed under "explicitly untouched"; that was a plan
  inaccuracy. Behavior untouched.
- The `layer_ops` paint-op vocabulary (`LayerPaintPlan`, `RectPaintOp`, etc.)
  and `text_layout`'s `TextAlign` / `TextWrapMode` are now publicly re-exported
  (required by §7.2's external-test assertions; smallest visibility change:
  re-exports, the modules themselves stay `pub(crate)`).
- `LayerPaintTarget` is publicly re-exported — a public writer seam must return
  a nameable type.
- G3's test scenario uses a normal-flow sibling so the root layer has content;
  the literal "only a layered child" document stages one apply (the root
  layer's plan is empty, per G5 semantics).

§9 open questions — resolved:

1. Accessor set: `with_staged_applies`, `with_staged_external_uploads`,
   `take_staged`.
2. Writer seam `acquire_paint_target_in_world`: **public**.
3. External RGBA: `handler: &RenderOpsHandler` parameter on
   `submit_external_rgba_in_world`; validation unchanged.
4. `RootDom::layer_lease(key)`: **public**.
5. Dead paint-revision channel **deleted** (zero callers workspace-wide);
   `DamagedRectsRepaint` machinery and intrinsic `generation` fields
   deliberately **untouched** (left for a future verdict, per §2.7).
6. **No new metric names.** `UploadDeferred` is incremented via a new
   `pub(crate)` `LayerPool::increment_metric`.
7. `RootDomContext::new_with_render_ops(config, Arc<RenderOpsHandler>)` +
   `render_ops()` reader + crate-internal `render_ops_shared()` for attach
   registration.

Gate status at landing: `cargo fmt --all --check`, `cargo clippy --workspace
--all-targets --all-features -- -D warnings`, and `cargo test --workspace` all
green. `cargo xtask lint` reports **406 pre-existing violations that also exist
on `main`** (zero added by this work) — so §8.7's "lint passes" criterion
cannot be literally satisfied until that pre-existing backlog is cleaned up;
flagged for the operator as a separate effort outside this plan's scope.
