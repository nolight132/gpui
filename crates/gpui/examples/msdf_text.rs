#![cfg_attr(target_family = "wasm", no_main)]

use std::borrow::Cow;

use gpui::{
    App, Bounds, Context, TitlebarOptions, Window, WindowBounds, WindowOptions, div, prelude::*,
    px, rgb, size,
};
use gpui_platform::application;

const IBM_PLEX: &[u8] =
    include_bytes!("../../../assets/fonts/ibm-plex-sans/IBMPlexSans-Regular.ttf");

struct MsdfTextExample {
    phase: f32,
}

impl Render for MsdfTextExample {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        self.phase = (self.phase + 0.008) % 1.0;
        window.request_animation_frame();

        let progress = 0.5 - 0.5 * (self.phase * std::f32::consts::TAU).cos();
        let embolden = px(-1.0 + 3.5 * progress);
        let specimen = "t tt with just a touch · w ww · A V O B 8 e g @ · а ф Ж · 漢字 · ◈ ◆ ✦";

        div()
            .flex()
            .flex_col()
            .size_full()
            .gap_6()
            .p_8()
            .bg(rgb(0x111318))
            .text_color(rgb(0xf4f5f7))
            .font_family("IBM Plex Sans")
            .child(
                div()
                    .text_2xl()
                    .child("Cross-platform raster and paint-only MTSDF text"),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(div().text_sm().child("Platform raster (unchanged)"))
                    .child(div().text_size(px(54.0)).child(specimen)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .child(format!("MTSDF optical embolden: {embolden:?}")),
                    )
                    // `msdf_text(px(0.0))` remains on this path too. Only the contour threshold
                    // below animates; shaping, advances, wrapping, FontId, and the atlas key stay
                    // stable.
                    .child(
                        div()
                            .text_size(px(54.0))
                            .msdf_text(embolden)
                            .child(specimen),
                    ),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(0x9ca3af))
                    .msdf_text(px(2.0))
                    .child("Explicit MSDF at 14 px intentionally falls back to hinted raster."),
            )
    }
}

fn run_example() {
    application().run(|cx: &mut App| {
        cx.text_system()
            .add_fonts(vec![Cow::Borrowed(IBM_PLEX)])
            .expect("embedded IBM Plex Sans should load");

        cx.open_window(
            WindowOptions {
                titlebar: Some(TitlebarOptions {
                    title: Some("GPUI MTSDF Example".into()),
                    ..Default::default()
                }),
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1050.0), px(520.0)),
                    cx,
                ))),
                ..Default::default()
            },
            |_, cx| cx.new(|_| MsdfTextExample { phase: 0.0 }),
        )
        .expect("MSDF example window should open");
    });
}

#[cfg(not(target_family = "wasm"))]
fn main() {
    run_example();
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    gpui_platform::web_init();
    run_example();
}
