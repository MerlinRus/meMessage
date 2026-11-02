use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::{
    Foundation::HWND,
    Graphics::Dwm::{DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute},
};

pub fn round_window(context: &eframe::CreationContext<'_>) -> Result<(), String> {
    let handle = context.window_handle().map_err(|e| e.to_string())?;
    if let RawWindowHandle::Win32(handle) = handle.as_raw() {
        let preference = DWMWCP_ROUND;
        // DWM clips the actual native window. The client must remain opaque.
        unsafe {
            DwmSetWindowAttribute(
                HWND(handle.hwnd.get() as *mut _),
                DWMWA_WINDOW_CORNER_PREFERENCE,
                std::ptr::from_ref(&preference).cast(),
                std::mem::size_of_val(&preference) as u32,
            )
            .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
