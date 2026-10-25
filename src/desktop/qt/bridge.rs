use super::WindowManager;
use std::panic::{AssertUnwindSafe, catch_unwind};

pub(super) struct DeepLinkDispatcher<'a> {
    pub(super) callback: &'a mut dyn FnMut(&str, &mut dyn WindowManager),
    pub(super) windows: &'a mut dyn WindowManager,
    pub(super) host: &'a QtHost,
    pub(super) callback_failed: bool,
}

fn dispatch_deep_link_callback(dispatcher: &mut DeepLinkDispatcher<'_>, uri: &str) {
    if catch_unwind(AssertUnwindSafe(|| {
        (dispatcher.callback)(uri, dispatcher.windows)
    }))
    .is_err()
    {
        dispatcher.callback_failed = true;
        dispatcher.host.quit();
    }
}

#[cxx::bridge(namespace = "kotavel::desktop")]
mod ffi {
    extern "Rust" {
        type DeepLinkDispatcher<'a>;
        fn dispatch_deep_link_callback(dispatcher: &mut DeepLinkDispatcher, uri: &str);
    }

    unsafe extern "C++" {
        include!("kotavel/src/desktop/qt/qt_host.h");

        type QtHost;

        fn create_qt_host(arguments: Vec<String>) -> Result<UniquePtr<QtHost>>;
        fn show(
            self: &QtHost,
            id: &str,
            title: &str,
            width: u32,
            height: u32,
            source: &str,
            icon: &str,
        ) -> Result<()>;
        fn close(self: &QtHost, id: &str);
        fn is_visible(self: &QtHost, id: &str) -> bool;
        fn has_visible_window(self: &QtHost) -> bool;
        fn exec(self: &QtHost, deep_link: &mut DeepLinkDispatcher) -> i32;
        fn quit(self: &QtHost);
    }
}

pub use ffi::{QtHost, create_qt_host};
