//! The screen kept on while a song plays (P51.3, issue #7: a Mac's display went dark mid-song, hands on the guitar;
//! Windows' did the same). Asked of the system only while wanted, and given back as soon as the song pauses or
//! stops, so the screen sleeps as usual otherwise. Windows: `SetThreadExecutionState` from the UI thread (the one
//! that asks keeps it; ES_CONTINUOUS until changed). Mac: an IOKit power assertion, "prevent idle display sleep".
//! Linux: nothing yet.

#[derive(Default)]
pub struct Awake {
    on: bool,
    #[cfg(target_os = "macos")]
    assertion: Option<u32>,
}

impl Awake {
    /// Keep the screen on (true) or let it sleep again (false). Cheap to call every frame: the system is asked only
    /// on a change.
    pub fn set(&mut self, want: bool) {
        if want == self.on {
            return;
        }
        self.on = want;
        self.apply(want);
    }

    #[cfg(windows)]
    fn apply(&mut self, want: bool) {
        use windows::Win32::System::Power::{
            ES_CONTINUOUS, ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED, SetThreadExecutionState,
        };
        let flags = if want {
            ES_CONTINUOUS | ES_DISPLAY_REQUIRED | ES_SYSTEM_REQUIRED
        } else {
            ES_CONTINUOUS
        };
        let before = unsafe { SetThreadExecutionState(flags) };
        crate::applog::write(&format!(
            "screen {} (Windows {})",
            if want {
                "kept on: a song is playing"
            } else {
                "may sleep again"
            },
            if before.0 != 0 { "agreed" } else { "refused" }
        ));
    }

    #[cfg(target_os = "macos")]
    fn apply(&mut self, want: bool) {
        if want {
            self.assertion = mac::prevent_display_sleep("quickRS: a song is playing");
            crate::applog::write(&format!(
                "screen kept on: a song is playing (macOS {})",
                if self.assertion.is_some() {
                    "agreed"
                } else {
                    "refused"
                }
            ));
        } else if let Some(id) = self.assertion.take() {
            mac::release(id);
            crate::applog::write("screen may sleep again");
        }
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    fn apply(&mut self, _want: bool) {}
}

impl Drop for Awake {
    fn drop(&mut self) {
        self.set(false);
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use std::ffi::{CString, c_char, c_void};

    type CFStringRef = *const c_void;
    const UTF8: u32 = 0x0800_0100; // kCFStringEncodingUTF8
    const LEVEL_ON: u32 = 255; // kIOPMAssertionLevelOn

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFStringCreateWithCString(alloc: *const c_void, s: *const c_char, encoding: u32) -> CFStringRef;
        fn CFRelease(cf: *const c_void);
    }

    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IOPMAssertionCreateWithName(kind: CFStringRef, level: u32, name: CFStringRef, id: *mut u32)
        -> i32;
        fn IOPMAssertionRelease(id: u32) -> i32;
    }

    fn cfstr(s: &str) -> Option<CFStringRef> {
        let c = CString::new(s).ok()?;
        let r = unsafe { CFStringCreateWithCString(std::ptr::null(), c.as_ptr(), UTF8) };
        (!r.is_null()).then_some(r)
    }

    /// An assertion that keeps the display from sleeping while the user is idle; its id (None: refused).
    pub fn prevent_display_sleep(reason: &str) -> Option<u32> {
        let kind = cfstr("PreventUserIdleDisplaySleep")?;
        let Some(name) = cfstr(reason) else {
            unsafe { CFRelease(kind) };
            return None;
        };
        let mut id = 0u32;
        let r = unsafe { IOPMAssertionCreateWithName(kind, LEVEL_ON, name, &mut id) };
        unsafe {
            CFRelease(name);
            CFRelease(kind);
        }
        (r == 0).then_some(id)
    }

    pub fn release(id: u32) {
        unsafe {
            IOPMAssertionRelease(id);
        }
    }
}
