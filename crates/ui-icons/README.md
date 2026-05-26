# ui-icons

Shared icon identifiers and atlas contract for flat UI icon rendering.

- Vendored Tabler outline SVG assets live under `assets/icons/vendor/tabler/outline/`
- The generated atlas lives as density-specific PNG+JSON pairs under `assets/icons/generated/ui-icons-x1.*`, `ui-icons-x2.*`, and `ui-icons-x4.*`
- `cargo run -p xtask -- icon-atlas` regenerates those artifacts from `UiIconId::ALL`

This crate stays pure-data so both apps and `xtask` can share the same icon
ordering without dragging Bevy or app-domain crates into the build step.
