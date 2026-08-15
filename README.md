# gpui

A fork of [GPUI](https://github.com/zed-industries/zed), the GPU-accelerated UI framework behind
Zed, extracted from the Zed workspace so it can move independently.

The fork adds effects the upstream framework does not have:

- `Styled::text_blur` — a gaussian blur baked into the glyph raster and cached in the atlas.
- `Styled::backdrop_blur` — CSS-style `backdrop-filter: blur()`, blurring whatever is already drawn.
- `Styled::blur` — CSS-style `filter: blur()`, blurring an element together with its subtree.
- `Window::paint_aurora` — animated colour fields for now-playing surfaces.

## Platforms

Only the Linux backends (`gpui_linux` + `gpui_wgpu`, Wayland and X11) are built today. The effects
live behind the same `PrimitiveBatch` interface every backend implements, so a Metal or DirectX
port is additive: handle the new batch kinds and translate the shaders. `gpui_platform` fails the
build with a clear message on platforms whose backend has not been ported yet.

## Licence

Apache-2.0, inherited from Zed. See `LICENSE-APACHE`.
