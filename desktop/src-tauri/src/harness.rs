//! Real mouse input for automated checks. A replay script's ordinary mouse
//! steps send events to the page, which is enough for the app's own
//! handlers but cannot make the web view select text: that only answers
//! to input arriving from the toolkit. These events are handed to GTK as
//! if they had come from the pointer, without moving the real one.

use glib::translate::ToGlibPtr;
use gtk::prelude::*;

/// Sends one mouse event to the web view at a point of the page, in the
/// page's own pixels; `ratio` is the page's device pixel ratio.
/// `kind` is `press`, `move` (with the button held) or `release`.
pub fn mouse(
    window: &tauri::WebviewWindow,
    kind: String,
    x: f64,
    y: f64,
    ratio: f64,
) -> Result<(), String> {
    window
        .with_webview(move |webview| {
            let view = webview.inner();
            // The toolkit counts in its own pixels: the page's, times
            // however much the page is zoomed beyond the screen's scale.
            let zoom = ratio / f64::from(view.scale_factor().max(1));
            let (x, y) = (x * zoom, y * zoom);
            // SAFETY: runs on the main thread, where GTK lives; the event
            // is built, dispatched and freed here and nowhere else.
            if let Err(e) = unsafe { send(&view, &kind, x, y) } {
                log::error!("Failed to send a mouse {kind}: {e}");
            }
        })
        .map_err(|e| format!("Failed to reach the web view: {e}"))
}

unsafe fn send(view: &webkit2gtk::WebView, kind: &str, x: f64, y: f64) -> Result<(), String> {
    let window = view.window().ok_or("The web view has no window yet")?;
    let pointer = window
        .display()
        .default_seat()
        .and_then(|seat| seat.pointer())
        .ok_or("The display has no pointer")?;
    // `move` drags with the button held; `hover` moves with none.
    let (kind, held) = match kind {
        "press" => (gdk::ffi::GDK_BUTTON_PRESS, gdk::ffi::GDK_BUTTON1_MASK),
        "release" => (gdk::ffi::GDK_BUTTON_RELEASE, gdk::ffi::GDK_BUTTON1_MASK),
        "move" => (gdk::ffi::GDK_MOTION_NOTIFY, gdk::ffi::GDK_BUTTON1_MASK),
        "hover" => (gdk::ffi::GDK_MOTION_NOTIFY, 0),
        other => return Err(format!("{other} is not a kind of mouse event")),
    };
    let time = (glib::monotonic_time() / 1000) as u32;
    unsafe {
        let event = gdk::ffi::gdk_event_new(kind);
        let target: *mut gdk::ffi::GdkWindow = window.to_glib_full();
        if kind == gdk::ffi::GDK_MOTION_NOTIFY {
            let motion = event as *mut gdk::ffi::GdkEventMotion;
            (*motion).window = target;
            (*motion).send_event = 1;
            (*motion).time = time;
            (*motion).x = x;
            (*motion).y = y;
            (*motion).x_root = x;
            (*motion).y_root = y;
            (*motion).state = held;
        } else {
            let button = event as *mut gdk::ffi::GdkEventButton;
            (*button).window = target;
            (*button).send_event = 1;
            (*button).time = time;
            (*button).x = x;
            (*button).y = y;
            (*button).x_root = x;
            (*button).y_root = y;
            (*button).button = 1;
            (*button).state = if kind == gdk::ffi::GDK_BUTTON_RELEASE {
                held
            } else {
                0
            };
        }
        gdk::ffi::gdk_event_set_device(event, pointer.to_glib_none().0);
        gtk::ffi::gtk_main_do_event(event);
        gdk::ffi::gdk_event_free(event);
    }
    Ok(())
}
