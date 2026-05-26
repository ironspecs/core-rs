# Icons

Vendored Tabler outline SVG sources live in `assets/icons/vendor/tabler/outline/`.

Flat UI icons are generated into density-specific PNG+JSON pairs under
`assets/icons/generated/` with:

```sh
cargo run -p xtask -- icon-atlas
```

The generated atlas is used for:
- control-app node header icons
- control-app inspector header icons
- future flat HUD icons in the runtime

`ui-dom` consumes the generated JSON geometry plus one matching PNG atlas for the
active retained pixel density. Internal flat icons are authored as normal
`img src="icon:<stem>"` nodes and resolve to typed `UiIconId` values before
paint.

World-space or zoom-aware SVG rendering remains a separate path.
