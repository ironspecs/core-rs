## Vendored Fonts

Shared UI fonts live under `assets/fonts/vendor/`.

Current set:
- `ibm-plex/IBMPlexSans-Variable.ttf`
- `ibm-plex/IBMPlexMono-Regular.ttf`
- `ibm-plex/IBMPlexMono-SemiBold.ttf`
- `source-han-sans/SourceHanSansJ-Regular.otf`
- `source-han-sans/SourceHanSansSC-Regular.otf`
- `source-han-sans/SourceHanSansK-Regular.otf`

Source:
- IBM Plex from the Google Fonts repository
- Source Han Sans from the official Adobe releases

Licensing:
- see `assets/fonts/vendor/ibm-plex/OFL.txt`
- upstream IBM license text is also preserved as `assets/fonts/vendor/ibm-plex/LICENSE.txt`
- Source Han Sans license text is preserved as `assets/fonts/vendor/source-han-sans/LICENSE.txt`

Usage:
- fixed UI text should load these through `ui-foundation::font_asset_path`
- control graph text uses the mono faces
- runtime shell and diagnostics use the sans/mono shared faces
- horizontal CJK paint proofs in `ui-dom` use the Source Han Sans J/SC/K regular faces
