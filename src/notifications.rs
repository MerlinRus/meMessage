use crate::model::{self, Message};
use std::{
    collections::{HashSet, VecDeque},
    path::PathBuf,
    sync::Arc,
};
use windows::{
    Data::Xml::Dom::XmlDocument,
    Foundation::TypedEventHandler,
    UI::Notifications::{
        NotificationSetting, ToastNotification, ToastNotificationManager, ToastNotifier,
    },
    Win32::{
        Foundation::PROPERTYKEY,
        System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize},
        System::{
            Com::{
                CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree, IPersistFile, STGM_READ,
                StructuredStorage::PROPVARIANT,
            },
            Variant::VT_LPWSTR,
        },
        UI::Shell::{
            FOLDERID_Programs, IShellLinkW, KF_FLAG_DEFAULT, PropertiesSystem::IPropertyStore,
            SHCNE_UPDATEITEM, SHCNF_PATHW, SHChangeNotify, SHGetKnownFolderPath, SHStrDupW,
            SetCurrentProcessExplicitAppUserModelID, ShellLink,
        },
    },
    core::{GUID, HSTRING, Interface},
};
use winreg::{RegKey, enums::HKEY_CURRENT_USER};

const APP_ID: &str = "MAARK.iPhoneBridge";
pub fn set_app_identity() -> Result<(), String> {
    let app = HSTRING::from(APP_ID);
    unsafe {
        SetCurrentProcessExplicitAppUserModelID(windows::core::PCWSTR(app.as_ptr()))
            .map_err(|e| e.to_string())
    }
}
pub struct Notifications {
    notifier: ToastNotifier,
    active: VecDeque<ToastNotification>,
}
impl Notifications {
    pub fn new() -> Result<Self, String> {
        unsafe {
            RoInitialize(RO_INIT_MULTITHREADED).map_err(|e| e.to_string())?;
        }
        let app = HSTRING::from(APP_ID);
        let registry = RegKey::predef(HKEY_CURRENT_USER);
        let (key, _) = registry
            .create_subkey(format!(r"Software\Classes\AppUserModelId\{APP_ID}"))
            .map_err(|e| e.to_string())?;
        key.set_value("DisplayName", &"iPhone Bridge")
            .map_err(|e| e.to_string())?;
        let icon = icon_path()?;
        key.set_value("IconUri", &icon.to_string_lossy().to_string())
            .map_err(|e| e.to_string())?;
        register_shortcut().map_err(|error| {
            format!("Не удалось зарегистрировать приложение для уведомлений: {error}")
        })?;
        let notifier =
            ToastNotificationManager::CreateToastNotifierWithId(&app).map_err(|e| e.to_string())?;
        Ok(Self {
            notifier,
            active: VecDeque::new(),
        })
    }
    fn ensure_enabled(&self) -> Result<(), String> {
        let setting = match self.notifier.Setting() {
            Ok(setting) => setting,
            // A new desktop identity can lack a settings entry until its first toast.
            // Show still respects Windows notification preferences.
            Err(error) if error.code().0 as u32 == 0x80070490 => return Ok(()),
            Err(error) => return Err(format!("Не удалось проверить уведомления Windows: {error}")),
        };
        match setting {
            NotificationSetting::Enabled => Ok(()),
            NotificationSetting::DisabledForApplication => {
                Err("Уведомления iPhone Bridge отключены в параметрах Windows.".into())
            }
            NotificationSetting::DisabledForUser => {
                Err("В Windows отключены уведомления для текущего пользователя.".into())
            }
            NotificationSetting::DisabledByGroupPolicy => {
                Err("Уведомления отключены политикой Windows.".into())
            }
            NotificationSetting::DisabledByManifest => {
                Err("Windows не распознала регистрацию уведомлений приложения.".into())
            }
            _ => Err(format!("Уведомления Windows недоступны ({}).", setting.0)),
        }
    }
    pub fn show(
        &mut self,
        title: &str,
        text: &str,
        click: Option<Arc<dyn Fn() + Send + Sync>>,
    ) -> Result<(), String> {
        self.ensure_enabled()?;
        let xml = toast_xml(title, text);
        let document = XmlDocument::new().map_err(|e| e.to_string())?;
        document
            .LoadXml(&HSTRING::from(xml))
            .map_err(|e| e.to_string())?;
        let toast =
            ToastNotification::CreateToastNotification(&document).map_err(|e| e.to_string())?;
        if let Some(click) = click {
            toast
                .Activated(&TypedEventHandler::new(move |_, _| {
                    click();
                    Ok(())
                }))
                .map_err(|e| e.to_string())?;
        }
        self.notifier.Show(&toast).map_err(|e| e.to_string())?;
        self.active.push_back(toast);
        while self.active.len() > 32 {
            self.active.pop_front();
        }
        Ok(())
    }
    pub fn history_count(&self) -> Result<u32, String> {
        ToastNotificationManager::History()
            .and_then(|history| history.GetHistoryWithId(&HSTRING::from(APP_ID)))
            .and_then(|history| history.Size())
            .map_err(|error| error.to_string())
    }
}
fn register_shortcut() -> Result<(), String> {
    // Classic desktop toasts require a Start menu shortcut with an explicit AUMID.
    // COM is initialized on the worker before this function is called.
    unsafe {
        let folder = SHGetKnownFolderPath(&FOLDERID_Programs, KF_FLAG_DEFAULT, None)
            .map_err(|e| e.to_string())?;
        let folder_text = folder.to_string();
        CoTaskMemFree(Some(folder.0.cast()));
        let path = PathBuf::from(folder_text.map_err(|e| e.to_string())?).join("iPhone Bridge.lnk");
        let root = model::data_path()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();
        let executable = HSTRING::from(root.join("iMessage.exe").as_os_str());
        let link: IShellLinkW =
            CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).map_err(|e| e.to_string())?;
        link.SetPath(&executable).map_err(|e| e.to_string())?;
        link.SetWorkingDirectory(&HSTRING::from(root.as_os_str()))
            .map_err(|e| e.to_string())?;
        link.SetIconLocation(&HSTRING::from(root.join("assets/app.ico").as_os_str()), 0)
            .map_err(|e| e.to_string())?;
        let store: IPropertyStore = link.cast().map_err(|e| e.to_string())?;
        let key = PROPERTYKEY {
            fmtid: GUID::from_u128(0x9f4c2855_9f79_4b39_a8d0_e1d42de1d5f3),
            pid: 5,
        };
        let mut value = PROPVARIANT::default();
        (*value.Anonymous.Anonymous).vt = VT_LPWSTR;
        // SHStrDupW allocates with CoTaskMem; PROPVARIANT's Drop frees that string.
        (*value.Anonymous.Anonymous).Anonymous.pwszVal =
            SHStrDupW(&HSTRING::from(APP_ID)).map_err(|e| e.to_string())?;
        store.SetValue(&key, &value).map_err(|e| e.to_string())?;
        store.Commit().map_err(|e| e.to_string())?;
        let file: IPersistFile = link.cast().map_err(|e| e.to_string())?;
        file.Save(&HSTRING::from(path.as_os_str()), true)
            .map_err(|e| e.to_string())?;
        let _ = repair_pinned_shortcuts(&root);
        let exe = HSTRING::from(root.join("iMessage.exe").as_os_str());
        SHChangeNotify(
            SHCNE_UPDATEITEM,
            SHCNF_PATHW,
            Some(exe.as_ptr().cast()),
            None,
        );
    }
    Ok(())
}
fn repair_pinned_shortcuts(root: &std::path::Path) -> Result<usize, String> {
    let Some(appdata) = std::env::var_os("APPDATA") else {
        return Ok(0);
    };
    let folder =
        PathBuf::from(appdata).join("Microsoft/Internet Explorer/Quick Launch/User Pinned/TaskBar");
    if !folder.exists() {
        return Ok(0);
    }
    let mut repaired = 0;
    for entry in std::fs::read_dir(folder).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path
            .extension()
            .is_none_or(|ext| !ext.eq_ignore_ascii_case("lnk"))
        {
            continue;
        }
        // Inspect links as files; repair only this application's own pinned shortcut.
        let result = (|| -> windows::core::Result<bool> {
            unsafe {
                let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
                let file: IPersistFile = link.cast()?;
                file.Load(&HSTRING::from(path.as_os_str()), STGM_READ)?;
                let store: IPropertyStore = link.cast()?;
                let key = PROPERTYKEY {
                    fmtid: GUID::from_u128(0x9f4c2855_9f79_4b39_a8d0_e1d42de1d5f3),
                    pid: 5,
                };
                let id = store
                    .GetValue(&key)
                    .ok()
                    .and_then(|value| windows::core::BSTR::try_from(&value).ok())
                    .map(|id| id.to_string());
                let mut target = [0u16; 32768];
                link.GetPath(&mut target, std::ptr::null_mut(), 4)?;
                let end = target.iter().position(|&c| c == 0).unwrap_or(target.len());
                let target = String::from_utf16_lossy(&target[..end]);
                let target = std::path::PathBuf::from(target);
                let belongs = target.starts_with(root)
                    && target.file_stem().is_some_and(|stem| {
                        let stem = stem.to_string_lossy().to_lowercase();
                        stem == "imessage"
                            || stem == "iphone-bridge"
                            || stem.starts_with("imessage-before-")
                    });
                if id.as_deref() != Some(APP_ID) && !belongs {
                    return Ok(false);
                }
                link.SetPath(&HSTRING::from(root.join("iMessage.exe").as_os_str()))?;
                link.SetWorkingDirectory(&HSTRING::from(root.as_os_str()))?;
                link.SetIconLocation(&HSTRING::from(root.join("assets/app.ico").as_os_str()), 0)?;
                let mut value = PROPVARIANT::default();
                (*value.Anonymous.Anonymous).vt = VT_LPWSTR;
                (*value.Anonymous.Anonymous).Anonymous.pwszVal = SHStrDupW(&HSTRING::from(APP_ID))?;
                store.SetValue(&key, &value)?;
                store.Commit()?;
                file.Save(&HSTRING::from(path.as_os_str()), true)?;
                let changed = HSTRING::from(path.as_os_str());
                SHChangeNotify(
                    SHCNE_UPDATEITEM,
                    SHCNF_PATHW,
                    Some(changed.as_ptr().cast()),
                    None,
                );
                Ok(true)
            }
        })();
        if result == Ok(true) {
            repaired += 1;
        }
    }
    Ok(repaired)
}
fn icon_path() -> Result<PathBuf, String> {
    let root = model::data_path()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("assets");
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let path = root.join("app-icon.png");
    if !path.exists() {
        let icon = crate::ui::app_icon();
        image::save_buffer(
            &path,
            &icon.rgba,
            icon.width,
            icon.height,
            image::ColorType::Rgba8,
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(path)
}
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
pub fn toast_xml(title: &str, text: &str) -> String {
    let body: String = text
        .chars()
        .filter(|&c| c == '\n' || c == '\t' || c >= ' ')
        .take(200)
        .collect();
    format!(
        r#"<toast><visual><binding template="ToastGeneric"><text>{}</text><text>{}</text></binding></visual><audio src="ms-winsoundevent:Notification.IM"/></toast>"#,
        escape(title),
        escape(&body)
    )
}
#[derive(Default)]
pub struct NotificationTracker {
    seen: HashSet<String>,
    initialized: bool,
}
impl NotificationTracker {
    pub fn observe<'a>(&mut self, messages: &'a [Message]) -> Vec<&'a Message> {
        let mut fresh = Vec::new();
        for message in messages {
            if self.seen.insert(message.id.clone()) && self.initialized && !message.outgoing {
                fresh.push(message);
            }
        }
        self.initialized = true;
        fresh
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SendState;
    fn message(id: &str, outgoing: bool) -> Message {
        Message {
            id: id.into(),
            handle: id.into(),
            phone: "+79990000000".into(),
            name: String::new(),
            text: "Тест".into(),
            timestamp: 1,
            outgoing,
            read: false,
            state: SendState::Received,
        }
    }
    #[test]
    fn notifications_only_for_new_incoming_after_baseline() {
        let mut tracker = NotificationTracker::default();
        assert!(tracker.observe(&[message("old", false)]).is_empty());
        let entries = vec![
            message("old", false),
            message("new", false),
            message("sent", true),
        ];
        let fresh = tracker.observe(&entries);
        assert_eq!(fresh.len(), 1);
        assert_eq!(fresh[0].id, "new");
        assert!(tracker.observe(&entries).is_empty());
    }
    #[test]
    fn escapes_xml_without_losing_emoji() {
        let xml = toast_xml("<Анна & я>", "Привет ❤️ <test>\0");
        assert!(xml.contains("&lt;Анна &amp; я&gt;"));
        assert!(xml.contains("Привет ❤️ &lt;test&gt;"));
        assert!(!xml.contains('\0'));
    }
}
