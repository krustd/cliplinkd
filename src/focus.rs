/// Focus detection module.
///
/// Returns:
/// - `Some(true)`  — the focused element is a text input field.
/// - `Some(false)` — something has focus but it's not a text input.
/// - `None`        — no focus detected, or the platform/API is unavailable.

pub fn is_focused_input() -> Option<bool> {
    #[cfg(target_os = "macos")]
    {
        macos::is_focused_input()
    }

    #[cfg(all(target_os = "linux", not(target_env = "musl")))]
    {
        // Try X11 first; fall back to AT-SPI2 (Wayland)
        if let Some(result) = linux_x11::is_focused_input() {
            return result;
        }
        linux_wayland::is_focused_input()
    }

    #[cfg(target_os = "windows")]
    {
        windows::is_focused_input()
    }

    #[cfg(not(any(
        target_os = "macos",
        target_os = "windows",
        all(target_os = "linux", not(target_env = "musl"))
    )))]
    {
        None
    }
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
            element: AXUIElementRef,
            attribute: *const c_void,
            value: *mut *const c_void,
        ) -> AXError;
        fn CFRelease(cf: *const c_void);
    }

    pub fn is_focused_input() -> Option<bool> {
        unsafe {
            if !AXIsProcessTrusted() {
                tracing::warn!("Accessibility permission not granted — focus detection disabled");
                return None;
            }

            let workspace: *mut objc2::runtime::AnyObject = objc2::msg_send![
                objc2::class!(NSWorkspace),
                sharedWorkspace
            ];
            let front_app: *mut objc2::runtime::AnyObject =
                objc2::msg_send![workspace, frontmostApplication];
            if front_app.is_null() {
                return None;
            }
            let pid: i32 = objc2::msg_send![front_app, processIdentifier];

            let app_ref = AXUIElementCreateApplication(pid);
            if app_ref.is_null() {
                return None;
            }

            let focused_attr = core_foundation::string::CFString::new("AXFocusedUIElement");
            let mut focused_ref: AXUIElementRef = std::ptr::null_mut();
            let ret = AXUIElementCopyAttributeValue(
                app_ref,
                focused_attr.as_concrete_TypeRef() as *const c_void,
                &mut focused_ref as *mut _ as *mut *const c_void,
            );
            CFRelease(app_ref as *const c_void);

            if ret != AX_SUCCESS || focused_ref.is_null() {
                return None;
            }

            let role_attr = core_foundation::string::CFString::new("AXRole");
            let mut role_value: *const c_void = std::ptr::null();
            let ret = AXUIElementCopyAttributeValue(
                focused_ref,
                role_attr.as_concrete_TypeRef() as *const c_void,
                &mut role_value as *mut _ as *mut *const c_void,
            );
            CFRelease(focused_ref as *const c_void);

            if ret != AX_SUCCESS || role_value.is_null() {
                return None;
            }

            let role_cf = core_foundation::string::CFString::wrap_under_create_rule(
                role_value as *const core_foundation::string::__CFString,
            );
            let role = role_cf.to_string();

            Some(role == "AXTextField" || role == "AXTextArea")
        }
    }
}

// ─── Linux X11 ───────────────────────────────────────────────────────────────

#[cfg(all(target_os = "linux", not(target_env = "musl")))]
mod linux_x11 {
    pub fn is_focused_input() -> Option<bool> {
        use x11::xlib;

        unsafe {
            let display = xlib::XOpenDisplay(std::ptr::null());
            if display.is_null() {
                return None; // X11 not available (likely Wayland)
            }

            let mut focus_window: xlib::Window = 0;
            let mut revert_to: i32 = 0;
            xlib::XGetInputFocus(display, &mut focus_window, &mut revert_to);

            if focus_window == 0 || focus_window == xlib::PointerRoot as u64 {
                xlib::XCloseDisplay(display);
                return None;
            }

            let mut class_hint = xlib::XClassHint {
                res_name: std::ptr::null_mut(),
                res_class: std::ptr::null_mut(),
            };
            let got_hint =
                xlib::XGetClassHint(display, focus_window, &mut class_hint) != 0;

            let result = if got_hint {
                let class = if !class_hint.res_class.is_null() {
                    Some(
                        std::ffi::CStr::from_ptr(class_hint.res_class)
                            .to_string_lossy(),
                    )
                } else {
                    None
                };
                let name = if !class_hint.res_name.is_null() {
                    Some(
                        std::ffi::CStr::from_ptr(class_hint.res_name)
                            .to_string_lossy(),
                    )
                } else {
                    None
                };

                let is_input = [class.as_deref(), name.as_deref()]
                    .iter()
                    .any(|s| matches_input_class(*s));

                if !class_hint.res_name.is_null() {
                    xlib::XFree(class_hint.res_name as *mut _);
                }
                if !class_hint.res_class.is_null() {
                    xlib::XFree(class_hint.res_class as *mut _);
                }

                Some(is_input)
            } else {
                Some(false)
            };

            xlib::XCloseDisplay(display);
            result
        }
    }

    fn matches_input_class(s: Option<&str>) -> bool {
        match s {
            Some(s) => {
                let s = s.to_lowercase();
                s.contains("entry")
                    || s.contains("input")
                    || s.contains("text")
                    || s.contains("edit")
                    || s.contains("terminal")
                    || s.contains("konsole")
                    || s.contains("kitty")
                    || s.contains("alacritty")
                    || s.contains("wezterm")
                    || s.contains("xterm")
                    || s.contains("gnome-terminal")
            }
            None => false,
        }
    }
}

// ─── Linux Wayland (AT-SPI2 via D-Bus) ───────────────────────────────────────

#[cfg(all(target_os = "linux", not(target_env = "musl")))]
mod linux_wayland {
    pub fn is_focused_input() -> Option<bool> {
        // Only attempt AT-SPI2 if we're plausibly on a Wayland session
        if !is_wayland_session() {
            return None;
        }

        let conn = zbus::blocking::Connection::session().ok()?;

        // AT-SPI2 Registry: Get the focused accessible
        let reply = conn
            .call_method(
                Some("org.a11y.Bus"),
                "/org/a11y/atspi/registry",
                Some("org.a11y.atspi.Registry"),
                "GetFocus",
                &(),
            )
            .ok()?;

        let body = reply.body();
        // GetFocus returns (s, o) — (app_name, accessible_path)
        let (_, path): (String, zbus::zvariant::OwnedObjectPath) =
            body.deserialize().ok()?;

        if path.as_str().is_empty() || path.as_str() == "/org/a11y/atspi/null" {
            return Some(false);
        }

        // Query the focused accessible's role
        let role_reply = conn
            .call_method(
                Some("org.a11y.Bus"),
                path.as_str(),
                Some("org.a11y.atspi.Accessible"),
                "GetRole",
                &(),
            )
            .ok()?;

        // GetRole returns u (uint32 role id)
        let role: u32 = role_reply.body().deserialize().ok()?;

        // AT-SPI2 role constants for text-input widgets
        // See: https://docs.gtk.org/atspi2/enum.Role.html
        Some(matches!(
            role,
            27 | // DOCUMENT_TEXT
            29 | // EDITBAR
            30 | // ENTRY
            34 | // PARAGRAPH
            42 | // TEXT
            44   // TERMINAL
        ))
    }

    fn is_wayland_session() -> bool {
        std::env::var("WAYLAND_DISPLAY").is_ok()
            || std::env::var("XDG_SESSION_TYPE")
                .map(|s| s == "wayland")
                .unwrap_or(false)
    }
}

// ─── Windows ─────────────────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
mod windows {
    use windows::Win32::UI::Accessibility::{
        CUIAutomation, IUIAutomation, IUIAutomationElement,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::GetGUIThreadInfo;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetForegroundWindow, GUITHREADINFO,
    };

    // UIA Control Type IDs (from Win32 UIAutomationClient.h)
    const UIA_EDIT_CONTROL_TYPE_ID: i32 = 50004;
    const UIA_TEXT_CONTROL_TYPE_ID: i32 = 50020;
    const UIA_DOCUMENT_CONTROL_TYPE_ID: i32 = 50034;
    const UIA_COMBOBOX_CONTROL_TYPE_ID: i32 = 50005;
    const UIA_IS_TEXT_PATTERN_AVAILABLE_PROPERTY_ID: i32 = 30040;

    pub fn is_focused_input() -> Option<bool> {
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.0 == 0 {
                return None;
            }

            let mut gti = GUITHREADINFO::default();
            gti.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;

            if GetGUIThreadInfo(0, &mut gti).is_err() || gti.hwndFocus.0 == 0 {
                return Some(false);
            }

            // ── Method 1: Class name detection (fast, no COM) ─────────
            let mut class_name = [0u16; 256];
            let len = GetClassNameW(gti.hwndFocus, &mut class_name);
            if len > 0 {
                let class_str =
                    String::from_utf16_lossy(&class_name[..len as usize]);
                let is_known_input = matches!(
                    class_str.as_str(),
                    "Edit"
                        | "RichEdit20A"
                        | "RichEdit20W"
                        | "RichEdit50W"
                        | "RICHEDIT50W"
                        | "Scintilla"
                );
                if is_known_input {
                    return Some(true);
                }
            }

            // ── Method 2: UIAutomation (Electron, UWP, WPF, Java) ─────
            check_uia_input(gti.hwndFocus)
        }
    }

    fn check_uia_input(
        hwnd: windows::Win32::Foundation::HWND,
    ) -> Option<bool> {
        unsafe {
            let automation: IUIAutomation =
                CUIAutomation::new().ok()?;

            let element: IUIAutomationElement =
                automation.ElementFromHandle(hwnd).ok()?;

            let control_type = element.CurrentControlType().unwrap_or(-1);

            let is_input_type = control_type == UIA_EDIT_CONTROL_TYPE_ID
                || control_type == UIA_TEXT_CONTROL_TYPE_ID
                || control_type == UIA_DOCUMENT_CONTROL_TYPE_ID
                || control_type == UIA_COMBOBOX_CONTROL_TYPE_ID;

            if is_input_type {
                return Some(true);
            }

            // Fallback: check IsTextPatternAvailable
            // Property value is VARIANT_BOOL → i16: -1 (true) or 0 (false)
            let prop = element
                .GetCurrentPropertyValue(UIA_IS_TEXT_PATTERN_AVAILABLE_PROPERTY_ID)
                .ok()?;
            let val: i16 = prop.try_into().ok()?;
            Some(val != 0)
        }
    }
}
