# `ui-dom`

`ui-dom` owns the retained DOM boundary used by the UI layers in this repository.

Shared generic collection mechanics such as LRU caches and fixed-size metrics
storage now live below this crate in `crates/collections`. `ui-dom` owns the
DOM-specific vocabulary and policy layered on top of those reusable
collections.

## Current Model

The crate is built around three root-owned concepts:

- `DomNode`: one node record stored in a root-owned slotmap
- `RootDom`: one attached document owner
- `RootDomFragment`: one detached staging owner

Both owners store `SlotMap<DomNodeSlotKey, DomNode>`. The difference is ownership and mutation role, not a separate node model.

## Node Kinds

`DomNode` currently has four variants:

- `Document`
- `DocumentFragment`
- `Element`
- `Text`

`Document` and `DocumentFragment` are the first node in their respective trees. They are container nodes. `Element` and `Text` are authored content nodes.

## Ownership Rules

- `RootDom` owns the authoritative attached document tree.
- `RootDomFragment` owns one detached staging tree.
- `DomNode` does not own the slotmap and cannot mutate tree structure by itself.
- Tree mutation authority belongs to the owning root type.
- `DomNodeSlotKey` is shared, but a key is only valid against the owner that stores it.

## Construction Rules

### `RootDom`

- `RootDom::new(context)` creates exactly one `Document` root node and binds that root to one shared `Arc<RootDomContext>`.
- `RootDomContext::shared_default()` returns one process-wide default context for tests and simple callers.
- `UiDomRenderPlugin::new(LayerConfig)` installs one fixed retained-layer density policy per Bevy app; non-text legacy families still flow through Vello scene submission, while retained text renders through a direct atlas-backed `wgpu` pass when `PaintBackend::Gpu` is selected.
- Callers explicitly create and append authored element and text nodes under that root.
- `root_key(&root_dom)` returns the `Document` node key.

### `RootDomFragment`

- `RootDomFragment::new()` creates exactly one `DocumentFragment` root node.
- Top-level staged children are appended under `document_fragment_key()`.
- The fragment may contain unreachable nodes in its slotmap, but only nodes reachable from the fragment root are imported.

## Fragment Import

`RootDom::append_fragment(parent_key, fragment)` and
`RootDomFragment::append_fragment(parent_key, fragment)` consume a
`RootDomFragment` and import the children of the fragment's
`DocumentFragment` root in order.

Import rules:

- the fragment root node itself is not imported
- descendant structure is preserved
- duplicate element ids fail before partial import
- unreachable fragment nodes are discarded when the fragment is dropped

## DOM-Shaped Traversal

Shared APIs should stay low-level and composable. Prefer normal traversal chains over wrapper-specific helpers.

```rust
root_dom
    .get_element_by_id("confirm-dialog-title")
    .and_then(|(_, node)| node.first_child(|key| root_dom.node(key)))
```

```rust
root_dom
    .node(parent_key)
    .and_then(|node| node.first_child(|key| root_dom.node(key)))
    .and_then(|(_, node)| node.next_sibling(|key| root_dom.node(key)))
```

## Keyboard Dispatch

`ui-dom` exposes one bridge to route Bevy keyboard events through DOM listeners
and root-owned default text behavior:

```rust
use ui_dom::dispatch_bevy_keyboard_input_from_focus_and_apply_default_behavior;
```

Callers pass the target `RootDom`, one `KeyboardInput`, and the current
`ButtonInput<KeyCode>` snapshot. Dispatch starts from the focused element when
present and falls back to the document root when not focused.

## Derived Domains

The authored tree drives the retained derived domains in `src/domains/`:

- layout
- paint
- composite
- hit test

Those domains read `RootDom` state and node-owned caches, but they do not become a second ownership model.

## GPU Boundary

`ui-dom` owns a direct retained `wgpu` execution boundary for its current paint families.

- Rect, text, image, and flat icon paint work all flow through direct retained GPU execution.
- Retained text atlas textures, bind groups, and instance buffers are prepared before render in the render app `PrepareResources` stage; the render pass only consumes that prepared GPU state.
- Flat icon atlases are generated ahead of time as density-specific PNG+JSON assets; the render executor owns the active atlas texture and validated manifest for its configured `LayerConfig.pixel_density`.
- Runtime SVG rasterization is not currently part of the boundary. World-space or zoom-aware SVG content remains a separate vector path.

## External Layer Target Proof

`RootDom::acquire_external_layer_target(...)` lets a caller acquire the Bevy
image handle composited for an already-presented DOM layer and render into it
without using `ui-dom` retained op shaders:

```rust
let target = root_dom.acquire_external_layer_target(world, node_key)?;
custom_renderer.render(world, &target)?;
```

Acquiring an external target switches the retained layer to external policy so
DOM-derived paint no longer overwrites the image.
CPU-authored RGBA content can use `LayerPool::submit_external_rgba_in_world`
to enqueue one full target upload for the render executor while keeping the
caller-owned source buffer retained outside `ui-dom`.

## Browser-Like Internal Model

`ui-dom` should be understood like a browser engine with an authored DOM input and retained derived pipelines.

- outsiders edit DOM nodes and DOM attributes
- outsiders also provide one Bevy parent entity as an anchor so `ui-dom` can place its presented output in the correct 2D/3D space
- Bevy is not the source of truth for layout, paint, or ordering decisions

Internally, DOM edits mark one or more retained dirty flags on nodes:

- `layout`
- `paint`
- `composite`

Those flags drive the internal sync pipelines on the next frame. The pipelines are separate on purpose:

- layout updates the retained geometric results for one layout tree
- paint draws directly into the Bevy image owned by one composite layer
- composite arranges owned layers relative to the foreign anchor parent and in the correct sibling order

The cheap steady-state rule is important:

- repeated syncs with no DOM edits should be very cheap
- if only paint changed, do not rerun layout
- if only composite changed, do not rerun layout or paint
- when there is a change, do the minimum pipeline work needed to make the presented layers correct again

## Visual Regression Artifacts

The crate-level visual suites now keep rolling artifacts beside the crate:

- `crates/ui-dom/tests/artifacts/<suite>/current`
- `crates/ui-dom/tests/artifacts/<suite>/previous`
- `crates/ui-dom/tests/artifacts/<suite>/diff`

The first run bootstraps `current` and records that no previous baseline
existed. Later runs rotate `current` to `previous`, write a fresh `current`,
and compare the new images to the immediately prior run.

## Composite-Layer Root Scheduling

The sync entrypoint should be composite-layer rooted.

- each composite layer is linked to one specific DOM node
- that DOM node identifies the layout tree whose layout and paint contribute to that layer image
- initial attach and later updates use the same layer-root layout schedule before paint and hit testing
- sync should start from those layer roots, inspect retained dirtiness there, and then run:
  - layout first when needed
  - paint second when needed
  - composite last when needed

That keeps the scheduling unit honest:

- DOM is still the authored editing model
- layer roots are the retained presentation scheduling roots
- first-frame presentation is not allowed to use stale pre-layout geometry for a presented layer root
- Bevy only presents the layer hierarchy that `ui-dom` has already decided
