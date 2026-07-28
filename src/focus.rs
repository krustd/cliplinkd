/// Focus detection module.
///
/// Returns `true` if the currently focused element can accept text input.

pub fn is_focused_input() -> bool {
    #[cfg(target_os = "macos")]
    { macos::detect() }

    #[cfg(all(target_os = "linux", not(target_env = "musl")))]
    {
        if linux_x11::detect() { return true; }
        linux_wayland::detect()
    }

    #[cfg(target_os = "windows")]
    { windows::detect() }

    #[cfg(not(any(
        target_os = "macos",
        target_os = "windows",
        all(target_os = "linux", not(target_env = "musl"))
    )))]
    { false }
}

// ─── macOS ───────────────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
mod macos {
    use core_foundation::base::TCFType;
    use std::ffi::c_void;

    type AXUIElementRef = *mut c_void;
    type AXError = i32;
    const AX_SUCCESS: AXError = 0;

    extern "C" {
        fn AXIsProcessTrusted() -> bool;
        fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
        fn AXUIElementCopyAttributeValue(
            element: AXUIElementRef, attribute: *const c_void, value: *mut *const c_void,
        ) -> AXError;
        fn CFRelease(cf: *const c_void);
    }

    pub fn detect() -> bool {
        unsafe {
            if !AXIsProcessTrusted() {
                return false;
            }

            let workspace: *mut objc2::runtime::AnyObject =
                objc2::msg_send![objc2::class!(NSWorkspace), sharedWorkspace];
            let front_app: *mut objc2::runtime::AnyObject =
                objc2::msg_send![workspace, frontmostApplication];
            if front_app.is_null() { return false; }

            // Check bundle ID for known terminal/editor apps
            let bundle_id: *mut objc2::runtime::AnyObject =
                objc2::msg_send![front_app, bundleIdentifier];
            if !bundle_id.is_null() {
                let utf8: *const i8 = objc2::msg_send![bundle_id, UTF8String];
                if !utf8.is_null() {
                    if is_known_app(&std::ffi::CStr::from_ptr(utf8).to_string_lossy()) {
                        return true;
                    }
                }
            }

            let pid: i32 = objc2::msg_send![front_app, processIdentifier];
            let app = AXUIElementCreateApplication(pid);
            if app.is_null() { return false; }

            let attr = core_foundation::string::CFString::new("AXFocusedUIElement");
            let mut focused: AXUIElementRef = std::ptr::null_mut();
            let r = AXUIElementCopyAttributeValue(app, attr.as_concrete_TypeRef() as *const c_void, &mut focused as *mut _ as *mut *const c_void);
            CFRelease(app as *const c_void);
            if r != AX_SUCCESS || focused.is_null() { return false; }

            let role_attr = core_foundation::string::CFString::new("AXRole");
            let mut role_val: *const c_void = std::ptr::null();
            let r = AXUIElementCopyAttributeValue(focused, role_attr.as_concrete_TypeRef() as *const c_void, &mut role_val as *mut _ as *mut *const c_void);
            CFRelease(focused as *const c_void);
            if r != AX_SUCCESS || role_val.is_null() { return false; }

            let role = core_foundation::string::CFString::wrap_under_create_rule(role_val as *const core_foundation::string::__CFString).to_string();
            matches!(role.as_str(), "AXTextField" | "AXTextArea" | "AXScrollArea" | "AXWebArea")
        }
    }

    fn is_known_app(id: &str) -> bool {
        matches!(id,
            "com.apple.Terminal" |
            "com.googlecode.iterm2" | "dev.warp.Warp-Stable" |
            "io.alacritty" | "net.kovidgoyal.kitty" | "org.wezfurlong.wezterm" |
            "com.microsoft.VSCode" | "com.cursor.Cursor" | "com.sublimetext.4" |
            "com.jetbrains.intellij" | "com.jetbrains.pycharm" | "com.jetbrains.goland" |
            "com.apple.dt.Xcode" | "md.obsidian" |
            "com.google.Chrome" | "com.apple.Safari" | "org.mozilla.firefox" |
            "com.brave.Browser" | "com.microsoft.edgemac" |
            "com.tencent.WeWorkMac" | "com.tencent.xinWeChat" |
            "com.tinyspeck.slackmacgap" | "com.hnc.Discord" | "ru.keepcoder.Telegram"
        )
    }
}

// ─── Linux X11 ───────────────────────────────────────────────────────────────

#[cfg(all(target_os = "linux", not(target_env = "musl")))]
mod linux_x11 {
    pub fn detect() -> bool {
        use x11::xlib;
        unsafe {
            let d = xlib::XOpenDisplay(std::ptr::null());
            if d.is_null() { return false; }

            let mut w: xlib::Window = 0;
            let mut rev: i32 = 0;
            xlib::XGetInputFocus(d, &mut w, &mut rev);
            if w == 0 || w == xlib::PointerRoot as u64 { xlib::XCloseDisplay(d); return false; }

            let mut h = xlib::XClassHint { res_name: std::ptr::null_mut(), res_class: std::ptr::null_mut() };
            let ok = xlib::XGetClassHint(d, w, &mut h) != 0;
            let result = if ok {
                let cls = ptr_str(h.res_class);
                let name = ptr_str(h.res_name);
                let r = cls.iter().chain(name.iter()).any(|s| {
                    let s = s.to_lowercase();
                    s.contains("entry") || s.contains("input") || s.contains("text") || s.contains("edit")
                    || s.contains("terminal") || s.contains("konsole") || s.contains("kitty")
                    || s.contains("alacritty") || s.contains("wezterm") || s.contains("xterm")
                });
                if !h.res_name.is_null() { xlib::XFree(h.res_name as *mut _); }
                if !h.res_class.is_null() { xlib::XFree(h.res_class as *mut _); }
                r
            } else { false };
            xlib::XCloseDisplay(d);
            result
        }
    }
    unsafe fn ptr_str(p: *mut i8) -> Option<String> {
        if p.is_null() { None } else { Some(std::ffi::CStr::from_ptr(p).to_string_lossy().into()) }
    }
}

// ─── Linux Wayland (AT-SPI2) ─────────────────────────────────────────────────

#[cfg(all(target_os = "linux", not(target_env = "musl")))]
mod linux_wayland {
    pub fn detect() -> bool {
        if !is_wayland() { return false; }
        let conn = match zbus::blocking::Connection::session() { Ok(c) => c, Err(_) => return false };
        let reply = match conn.call_method(Some("org.a11y.Bus"), "/org/a11y/atspi/registry", Some("org.a11y.atspi.Registry"), "GetFocus", &()) { Ok(r) => r, Err(_) => return false };
        let (_, path): (String, zbus::zvariant::OwnedObjectPath) = match reply.body().deserialize() { Ok(v) => v, Err(_) => return false };
        if path.as_str().is_empty() || path.as_str() == "/org/a11y/atspi/null" { return false; }
        let rr = match conn.call_method(Some("org.a11y.Bus"), path.as_str(), Some("org.a11y.atspi.Accessible"), "GetRole", &()) { Ok(r) => r, Err(_) => return false };
        let role: u32 = match rr.body().deserialize() { Ok(v) => v, Err(_) => return false };
        matches!(role, 27 | 29 | 30 | 34 | 42 | 44)
    }
    fn is_wayland() -> bool {
        std::env::var("WAYLAND_DISPLAY").is_ok() || std::env::var("XDG_SESSION_TYPE").map(|s| s == "wayland").unwrap_or(false)
    }
}

// ─── Windows ─────────────────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
mod windows {
    use windows::Win32::UI::Accessibility::{CUIAutomation, IUIAutomation, IUIAutomationElement};
    use windows::Win32::UI::Input::KeyboardAndMouse::GetGUIThreadInfo;
    use windows::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetForegroundWindow, GUITHREADINFO};

    pub fn detect() -> bool {
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.0 == 0 { return false; }
            let mut gti = GUITHREADINFO::default();
            gti.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;
            if GetGUIThreadInfo(0, &mut gti).is_err() || gti.hwndFocus.0 == 0 { return false; }

            let mut cn = [0u16; 256];
            let len = GetClassNameW(gti.hwndFocus, &mut cn);
            if len > 0 {
                let s = String::from_utf16_lossy(&cn[..len as usize]);
                if matches!(s.as_str(), "Edit" | "RichEdit20A" | "RichEdit20W" | "RichEdit50W" | "RICHEDIT50W" | "Scintilla") {
                    return true;
                }
            }

            if let Some(r) = uia_check(gti.hwndFocus) { return r; }
            false
        }
    }

    fn uia_check(hwnd: windows::Win32::Foundation::HWND) -> Option<bool> {
        unsafe {
            let a: IUIAutomation = CUIAutomation::new().ok()?;
            let el: IUIAutomationElement = a.ElementFromHandle(hwnd).ok()?;
            let ct = el.CurrentControlType().unwrap_or(-1);
            if matches!(ct, 50004 | 50020 | 50034 | 50005) { return Some(true); }
            let prop = el.GetCurrentPropertyValue(30040).ok()?;
            let v: i16 = prop.try_into().ok()?;
            Some(v != 0)
        }
    }
}
