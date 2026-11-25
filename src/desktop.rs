//! Small Windows shell integration. The tray owns its HWND, icons and COM objects
//! on one message-loop thread; egui receives actions through a channel.
use eframe::egui::{Context, ViewportCommand};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{
    io::Cursor,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    time::Duration,
};
use windows::{
    Win32::{
        Foundation::{
            CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND, LPARAM, LRESULT, POINT,
            WPARAM,
        },
        System::{
            Com::{
                CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
                CoUninitialize,
            },
            LibraryLoader::GetModuleHandleW,
            Threading::CreateMutexW,
        },
        UI::{Shell::*, WindowsAndMessaging::*},
    },
    core::{HSTRING, w},
};
use winreg::{RegKey, enums::HKEY_CURRENT_USER};

const CALLBACK: u32 = WM_APP + 101;
const UPDATE: u32 = WM_APP + 102;
const OPEN: u32 = WM_APP + 103;
const READD: u32 = WM_APP + 104;
const CLASS: windows::core::PCWSTR = w!("MAARK.meMessage.Tray");
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

pub struct Instance(HANDLE);
impl Instance {
    pub fn acquire() -> Result<Option<Self>, String> {
        unsafe {
            let handle = CreateMutexW(None, true, w!("Local\\MAARK.meMessage"))
                .map_err(|e| e.to_string())?;
            if GetLastError() != ERROR_ALREADY_EXISTS {
                return Ok(Some(Self(handle)));
            }
            let _ = CloseHandle(handle);
            for _ in 0..40 {
                if let Ok(window) = FindWindowW(CLASS, None) {
                    let _ = PostMessageW(Some(window), OPEN, WPARAM(0), LPARAM(0));
                    return Ok(None);
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err("meMessage уже запускается. Подожди несколько секунд и открой его снова.".into())
        }
    }
}
impl Drop for Instance {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

pub fn autostart_command(exe: &std::path::Path) -> String {
    format!("\"{}\" --background", exe.display())
}
pub fn autostart_enabled() -> bool {
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(RUN_KEY)
        .ok()
        .and_then(|key| key.get_value::<String, _>("meMessage").ok())
        .is_some_and(|value| {
            value == autostart_command(&crate::model::app_root().join("iMessage.exe"))
        })
}
pub fn set_autostart(enabled: bool) -> Result<(), String> {
    let (key, _) = RegKey::predef(HKEY_CURRENT_USER)
        .create_subkey(RUN_KEY)
        .map_err(|e| e.to_string())?;
    if enabled {
        key.set_value(
            "meMessage",
            &autostart_command(&crate::model::app_root().join("iMessage.exe")),
        )
        .map_err(|e| e.to_string())
    } else {
        match key.delete_value("meMessage") {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.to_string()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Open,
    Settings,
    Exit,
    Resume,
}
pub struct Desktop {
    hwnd: isize,
    pub events: Receiver<Action>,
    alive: Arc<AtomicBool>,
    last: Option<(usize, bool)>,
}
impl Desktop {
    pub fn start(cc: &eframe::CreationContext<'_>) -> Result<Self, String> {
        let raw = cc.window_handle().map_err(|e| e.to_string())?;
        let RawWindowHandle::Win32(raw) = raw.as_raw() else {
            return Err("Нужно окно Windows.".into());
        };
        Self::spawn(cc.egui_ctx.clone(), raw.hwnd.get())
    }
    fn spawn(ctx: Context, main: isize) -> Result<Self, String> {
        let (events_tx, events) = mpsc::channel();
        let (ready_tx, ready) = mpsc::sync_channel(1);
        let alive = Arc::new(AtomicBool::new(false));
        let running = alive.clone();
        std::thread::spawn(move || {
            let result = unsafe { run(ctx, main, events_tx, &ready_tx, running.clone()) };
            if let Err(error) = result {
                let _ = ready_tx.send(Err(error));
            }
            running.store(false, Ordering::Release);
        });
        let hwnd = ready
            .recv_timeout(Duration::from_secs(5))
            .map_err(|e| e.to_string())??;
        Ok(Self {
            hwnd,
            events,
            alive,
            last: None,
        })
    }
    pub fn available(&self) -> bool {
        self.alive.load(Ordering::Acquire)
    }
    pub fn update(&mut self, unread: usize, connected: bool) {
        if self.last != Some((unread, connected)) && self.available() {
            let sent = unsafe {
                PostMessageW(
                    Some(HWND(self.hwnd as *mut _)),
                    UPDATE,
                    WPARAM(unread),
                    LPARAM(isize::from(connected)),
                )
            };
            if sent.is_ok() {
                self.last = Some((unread, connected));
            }
        }
    }
}
impl Drop for Desktop {
    fn drop(&mut self) {
        unsafe {
            let _ = PostMessageW(
                Some(HWND(self.hwnd as *mut _)),
                WM_CLOSE,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }
}

struct WindowActions {
    ctx: Context,
    events: mpsc::Sender<Action>,
    taskbar_created: u32,
}
impl WindowActions {
    fn action(&self, action: Action) {
        if matches!(action, Action::Open | Action::Settings | Action::Exit) {
            self.ctx.send_viewport_cmd(ViewportCommand::Visible(true));
            self.ctx
                .send_viewport_cmd(ViewportCommand::Minimized(false));
            self.ctx.send_viewport_cmd(ViewportCommand::Focus);
        }
        let _ = self.events.send(action);
        self.ctx.request_repaint();
    }
}
struct State {
    main: HWND,
    data: NOTIFYICONDATAW,
    icon: HICON,
    overlay: HICON,
    taskbar: Option<ITaskbarList3>,
    unread: usize,
    connected: bool,
}
impl State {
    unsafe fn add(&mut self) -> bool {
        unsafe {
            if !Shell_NotifyIconW(NIM_ADD, &self.data).as_bool() {
                return false;
            }
            self.data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
            Shell_NotifyIconW(NIM_SETVERSION, &self.data).as_bool()
        }
    }
    unsafe fn update(&mut self, count: usize, connected: bool) {
        unsafe {
            if count != self.unread || self.icon.is_invalid() {
                if let Ok(icon) = icon(count, false) {
                    let previous = self.icon;
                    self.icon = icon;
                    self.data.hIcon = icon;
                    let _ = Shell_NotifyIconW(NIM_MODIFY, &self.data);
                    if !previous.is_invalid() {
                        let _ = DestroyIcon(previous);
                    }
                }
                if !self.overlay.is_invalid() {
                    let _ = DestroyIcon(self.overlay);
                    self.overlay = HICON::default();
                }
                if count > 0 {
                    self.overlay = icon(count, true).unwrap_or_default();
                }
            }
            self.unread = count;
            self.connected = connected;
            self.data.szTip.fill(0);
            let text = format!(
                "meMessage · {} непрочитанных · {}",
                count,
                if connected {
                    "iPhone подключён"
                } else {
                    "нет связи с iPhone"
                }
            );
            for (out, value) in self
                .data
                .szTip
                .iter_mut()
                .take(127)
                .zip(text.encode_utf16())
            {
                *out = value;
            }
            let _ = Shell_NotifyIconW(NIM_MODIFY, &self.data);
            if let Some(taskbar) = &self.taskbar {
                let _ = taskbar.SetOverlayIcon(
                    self.main,
                    self.overlay,
                    &HSTRING::from(format!("Непрочитанных сообщений: {count}")),
                );
            }
        }
    }
}
impl Drop for State {
    fn drop(&mut self) {
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &self.data);
            if let Some(taskbar) = &self.taskbar {
                let _ = taskbar.SetOverlayIcon(self.main, HICON::default(), w!(""));
            }
            if !self.icon.is_invalid() {
                let _ = DestroyIcon(self.icon);
            }
            if !self.overlay.is_invalid() {
                let _ = DestroyIcon(self.overlay);
            }
        }
    }
}

unsafe fn run(
    ctx: Context,
    main: isize,
    events: mpsc::Sender<Action>,
    ready: &mpsc::SyncSender<Result<isize, String>>,
    alive: Arc<AtomicBool>,
) -> Result<(), String> {
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED)
            .ok()
            .map_err(|e| e.to_string())?;
        struct Com;
        impl Drop for Com {
            fn drop(&mut self) {
                unsafe {
                    CoUninitialize();
                }
            }
        }
        let _com = Com;
        let instance = GetModuleHandleW(None).map_err(|e| e.to_string())?;
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance.into(),
            lpszClassName: CLASS,
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            return Err("Не удалось зарегистрировать значок meMessage.".into());
        }
        // WndProc can re-enter during shell/COM calls. Its immutable callback
        // data must not alias the mutable icon state used by the message loop.
        let actions = Box::new(WindowActions {
            ctx,
            events,
            taskbar_created: RegisterWindowMessageW(w!("TaskbarCreated")),
        });
        let mut state = Box::new(State {
            main: HWND(main as *mut _),
            data: NOTIFYICONDATAW::default(),
            icon: HICON::default(),
            overlay: HICON::default(),
            taskbar: CoCreateInstance::<_, ITaskbarList3>(&TaskbarList, None, CLSCTX_INPROC_SERVER)
                .ok(),
            unread: usize::MAX,
            connected: false,
        });
        if let Some(taskbar) = &state.taskbar {
            let _ = taskbar.HrInit();
        }
        let window = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            CLASS,
            w!("meMessage tray"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance.into()),
            Some((&*actions as *const WindowActions).cast()),
        )
        .map_err(|e| e.to_string())?;
        struct Window(HWND);
        impl Drop for Window {
            fn drop(&mut self) {
                unsafe {
                    let _ = DestroyWindow(self.0);
                }
            }
        }
        let _window = Window(window);
        state.data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        state.data.hWnd = window;
        state.data.uID = 1;
        state.data.uCallbackMessage = CALLBACK;
        state.data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP;
        state.icon = icon(0, false)?;
        state.data.hIcon = state.icon;
        for (out, value) in state.data.szTip.iter_mut().zip("meMessage".encode_utf16()) {
            *out = value;
        }
        if !state.add() {
            let _ = DestroyWindow(window);
            return Err("Windows не создала значок в трее. Окно останется доступным.".into());
        }
        alive.store(true, Ordering::Release);
        if ready.send(Ok(window.0 as isize)).is_err() {
            let _ = DestroyWindow(window);
            return Ok(());
        }
        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).0 > 0 {
            if message.message == UPDATE {
                state.update(message.wParam.0, message.lParam.0 != 0);
                continue;
            }
            if message.message == READD {
                state.add();
                state.update(state.unread, state.connected);
                continue;
            }
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        Ok(())
    }
}
unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        if message == WM_NCCREATE {
            let create = &*(lparam.0 as *const CREATESTRUCTW);
            SetWindowLongPtrW(window, GWLP_USERDATA, create.lpCreateParams as isize);
        }
        let pointer = GetWindowLongPtrW(window, GWLP_USERDATA) as *const WindowActions;
        if !pointer.is_null() {
            let state = &*pointer;
            if message == state.taskbar_created && state.taskbar_created != 0 {
                let _ = PostMessageW(Some(window), READD, WPARAM(0), LPARAM(0));
                return LRESULT(0);
            }
            match message {
                OPEN => {
                    state.action(Action::Open);
                    return LRESULT(0);
                }
                WM_POWERBROADCAST if wparam.0 == 18 => {
                    state.action(Action::Resume);
                    return LRESULT(1);
                }
                CALLBACK => {
                    let event = lparam.0 as u32 & 0xffff;
                    if event == NIN_SELECT || event == (NIN_SELECT | 1) || event == WM_LBUTTONDBLCLK
                    {
                        state.action(Action::Open);
                    }
                    if event == WM_CONTEXTMENU
                        && let Ok(menu) = CreatePopupMenu()
                    {
                        let _ = AppendMenuW(menu, MF_STRING, 1, w!("Открыть meMessage"));
                        let _ = AppendMenuW(menu, MF_STRING, 2, w!("Настройки"));
                        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
                        let _ = AppendMenuW(menu, MF_STRING, 3, w!("Выйти"));
                        let mut point = POINT::default();
                        let _ = GetCursorPos(&mut point);
                        let _ = SetForegroundWindow(window);
                        let selected = TrackPopupMenu(
                            menu,
                            TPM_RETURNCMD | TPM_RIGHTBUTTON,
                            point.x,
                            point.y,
                            Some(0),
                            window,
                            None,
                        )
                        .0;
                        let _ = DestroyMenu(menu);
                        match selected {
                            1 => state.action(Action::Open),
                            2 => state.action(Action::Settings),
                            3 => state.action(Action::Exit),
                            _ => {}
                        }
                        let _ = PostMessageW(Some(window), WM_NULL, WPARAM(0), LPARAM(0));
                    }
                    return LRESULT(0);
                }
                WM_CLOSE => {
                    let _ = DestroyWindow(window);
                    return LRESULT(0);
                }
                WM_DESTROY => {
                    PostQuitMessage(0);
                    return LRESULT(0);
                }
                _ => {}
            }
        }
        DefWindowProcW(window, message, wparam, lparam)
    }
}

fn badge(count: usize) -> image::RgbaImage {
    const DIGITS: [[u8; 5]; 11] = [
        [7, 5, 5, 5, 7],
        [2, 6, 2, 2, 7],
        [7, 1, 7, 4, 7],
        [7, 1, 7, 1, 7],
        [5, 5, 7, 1, 1],
        [7, 4, 7, 1, 7],
        [7, 4, 7, 5, 7],
        [7, 1, 1, 1, 1],
        [7, 5, 7, 5, 7],
        [7, 5, 7, 1, 7],
        [0, 2, 7, 2, 0],
    ];
    let mut image = image::RgbaImage::new(16, 16);
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        if (x as f32 - 7.5).hypot(y as f32 - 7.5) < 8. {
            *pixel = image::Rgba([235, 65, 58, 255]);
        }
    }
    let text = if count > 99 {
        "99+".into()
    } else {
        count.to_string()
    };
    let left = (16 - (text.len() as u32 * 4 - 1)) / 2;
    for (i, digit) in text.bytes().enumerate() {
        let glyph = if digit == b'+' {
            10
        } else {
            (digit - b'0') as usize
        };
        for (y, bits) in DIGITS[glyph].iter().enumerate() {
            for x in 0..3 {
                if bits & (1 << (2 - x)) != 0 {
                    image.put_pixel(
                        left + i as u32 * 4 + x,
                        5 + y as u32,
                        image::Rgba([255, 255, 255, 255]),
                    );
                }
            }
        }
    }
    image
}
fn icon(count: usize, overlay: bool) -> Result<HICON, String> {
    let rgba = if overlay {
        badge(count)
    } else {
        let app = crate::ui::app_icon();
        let base = image::RgbaImage::from_raw(app.width, app.height, app.rgba)
            .ok_or("Неверный размер значка")?;
        let mut base =
            image::imageops::resize(&base, 32, 32, image::imageops::FilterType::Triangle);
        if count > 0 {
            image::imageops::overlay(&mut base, &badge(count), 16, 16);
        }
        base
    };
    let side = rgba.width();
    let mut png = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(rgba)
        .write_to(&mut png, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    unsafe {
        CreateIconFromResourceEx(
            png.get_ref(),
            true,
            0x30000,
            side as i32,
            side as i32,
            LR_DEFAULTCOLOR,
        )
        .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_tray_opens_on_activation_resumes_and_cleans_up_its_thread() {
        let ctx = Context::default();
        let mut desktop = Desktop::spawn(ctx.clone(), 0).unwrap();
        assert!(desktop.available());
        desktop.update(12, true);
        let window = HWND(desktop.hwnd as *mut _);
        unsafe {
            PostMessageW(
                Some(window),
                CALLBACK,
                WPARAM(0),
                LPARAM(NIN_SELECT as isize),
            )
            .unwrap();
        }
        assert_eq!(
            desktop.events.recv_timeout(Duration::from_secs(2)).unwrap(),
            Action::Open
        );
        let output = ctx.run_logic(&eframe::egui::RawInput::default(), |_| {});
        assert!(
            output.viewport_commands[&eframe::egui::ViewportId::ROOT]
                .contains(&ViewportCommand::Visible(true))
        );
        unsafe {
            PostMessageW(Some(window), WM_POWERBROADCAST, WPARAM(18), LPARAM(0)).unwrap();
        }
        assert_eq!(
            desktop.events.recv_timeout(Duration::from_secs(2)).unwrap(),
            Action::Resume
        );
        let alive = desktop.alive.clone();
        drop(desktop);
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while alive.load(Ordering::Acquire) && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!alive.load(Ordering::Acquire));
    }
    #[test]
    fn badge_handles_zero_and_large_counts_and_native_icon_creation() {
        for count in [0, 1, 9, 10, 99, 100, usize::MAX] {
            assert!(badge(count).pixels().any(|p| p.0 == [255, 255, 255, 255]));
            for overlay in [true, false] {
                let handle = icon(count, overlay).unwrap();
                unsafe {
                    DestroyIcon(handle).unwrap();
                }
            }
        }
        assert_eq!(
            autostart_command(std::path::Path::new(r"D:\МАРК\Project space\iMessage.exe")),
            r#""D:\МАРК\Project space\iMessage.exe" --background"#
        );
    }
}
