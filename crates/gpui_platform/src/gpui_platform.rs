//! Convenience crate that re-exports GPUI's platform traits and the
//! `current_platform` constructor so consumers don't need `#[cfg]` gating.

pub use gpui::Platform;

use std::rc::Rc;

/// Returns a background executor for the current platform.
pub fn background_executor() -> gpui::BackgroundExecutor {
    current_platform(true).background_executor()
}

pub fn application() -> gpui::Application {
    gpui::Application::with_platform(current_platform(false))
}

pub fn headless() -> gpui::Application {
    gpui::Application::with_platform(current_platform(true))
}

/// Returns the default [`Platform`] for the current OS.
pub fn current_platform(headless: bool) -> Rc<dyn Platform> {
    #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
    compile_error!(
        "this fork only carries the Linux backends so far; port a renderer and add it here"
    );

    gpui_linux::current_platform(headless)
}

/// Returns a new [`HeadlessRenderer`] for the current platform, if available.
#[cfg(feature = "test-support")]
pub fn current_headless_renderer() -> Option<Box<dyn gpui::PlatformHeadlessRenderer>> {
    None
}
