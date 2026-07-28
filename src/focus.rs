/// Minimal focus detection — just checks if the frontmost app is a known
/// text-capable app via bundle ID. Always returns true on non-macOS.
/// This is only used for status reporting; paste ALWAYS happens regardless.
pub fn is_focused_input() -> bool {
    #[cfg(target_os = "macos")]
    {
        // Quick bundle ID check — covers 95% of paste-able apps
        use objc2::runtime::AnyObject;
        unsafe {
            let workspace: *mut AnyObject =
                objc2::msg_send![objc2::class!(NSWorkspace), sharedWorkspace];
            let app: *mut AnyObject = objc2::msg_send![workspace, frontmostApplication];
            if app.is_null() { return true; } // can't detect, assume yes
            let bundle: *mut AnyObject = objc2::msg_send![app, bundleIdentifier];
            if bundle.is_null() { return true; }
            let utf8: *const i8 = objc2::msg_send![bundle, UTF8String];
            if utf8.is_null() { return true; }
            let id = std::ffi::CStr::from_ptr(utf8).to_string_lossy();
            matches!(id.as_ref(),
                "com.apple.Terminal" | "com.googlecode.iterm2" | "dev.warp.Warp-Stable" |
                "io.alacritty" | "net.kovidgoyal.kitty" | "org.wezfurlong.wezterm" |
                "com.microsoft.VSCode" | "com.cursor.Cursor" | "com.sublimetext.4" |
                "com.jetbrains.intellij" | "com.jetbrains.pycharm" | "com.jetbrains.goland" |
                "com.apple.dt.Xcode" | "md.obsidian" |
                "com.google.Chrome" | "com.apple.Safari" | "org.mozilla.firefox" |
                "com.brave.Browser" | "com.microsoft.edgemac" |
                "com.tencent.WeWorkMac" | "com.tencent.xinWeChat" |
                "com.tinyspeck.slackmacgap" | "com.hnc.Discord" | "ru.keepcoder.Telegram" |
                "com.apple.TextEdit" | "com.apple.Notes" | "com.apple.mail"
            )
        }
    }

    #[cfg(not(target_os = "macos"))]
    { true }
}
