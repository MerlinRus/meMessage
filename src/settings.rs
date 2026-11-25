use crate::model;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    io,
    path::Path,
};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChatSettings {
    pub pinned: bool,
    pub muted: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub auto_focus: bool,
    pub animations: bool,
    pub notifications: bool,
    pub notification_sound: bool,
    pub poll_seconds: u64,
    pub sync_history_on_connect: bool,
    pub save_history: bool,
    pub save_drafts: bool,
    pub minimize_on_close: bool,
    pub autostart: bool,
    pub notification_preview: bool,
    pub chats: BTreeMap<String, ChatSettings>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            auto_focus: true,
            animations: true,
            notifications: true,
            notification_sound: true,
            poll_seconds: 3,
            sync_history_on_connect: true,
            save_history: true,
            save_drafts: true,
            minimize_on_close: false,
            autostart: false,
            notification_preview: true,
            chats: BTreeMap::new(),
        }
    }
}
impl Settings {
    pub fn normalize(&mut self) {
        self.poll_seconds = self.poll_seconds.clamp(2, 60);
    }
    pub fn load(path: &Path) -> io::Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let mut settings: Self = read_private(path)?;
        settings.normalize();
        Ok(settings)
    }
    pub fn save(&self, path: &Path) -> io::Result<()> {
        write_private(path, self)
    }
}
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Drafts {
    pub messages: HashMap<String, String>,
    pub selected: Option<String>,
    pub new_mode: bool,
    pub recipient: String,
    pub uncertain_submission: bool,
    pub submitted: Option<(String, String)>,
}
impl Drafts {
    pub fn load(path: &Path) -> io::Result<Self> {
        if path.exists() {
            read_private(path)
        } else {
            Ok(Self::default())
        }
    }
    pub fn save(&self, path: &Path) -> io::Result<()> {
        write_private(path, self)
    }
}
pub fn read_private<T: serde::de::DeserializeOwned>(path: &Path) -> io::Result<T> {
    serde_json::from_slice(&model::protect(&std::fs::read(path)?, false)?).map_err(io::Error::other)
}
pub fn write_private<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let bytes = model::protect(&serde_json::to_vec(value)?, true)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, bytes)?;
    std::fs::rename(temporary, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_are_backward_compatible_and_period_is_bounded() {
        let mut s: Settings = serde_json::from_str(r#"{"poll_seconds":0}"#).unwrap();
        s.normalize();
        assert_eq!(s.poll_seconds, 2);
        assert!(s.auto_focus && s.save_history && s.save_drafts);
        s.poll_seconds = 999;
        s.normalize();
        assert_eq!(s.poll_seconds, 60);
        assert!(s.chats.is_empty());
        assert!(s.notification_preview);
        assert!(!s.autostart);
    }
    #[test]
    fn settings_and_drafts_survive_encrypted_storage() {
        let root = std::env::temp_dir().join(format!("meMessage-settings-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let config = root.join("settings.bin");
        let s = Settings {
            auto_focus: false,
            poll_seconds: 12,
            notification_preview: false,
            chats: BTreeMap::from([(
                "+79991234567".into(),
                ChatSettings {
                    pinned: true,
                    muted: true,
                },
            )]),
            ..Default::default()
        };
        s.save(&config).unwrap();
        assert_eq!(Settings::load(&config).unwrap(), s);
        let path = root.join("drafts.bin");
        let d = Drafts {
            messages: HashMap::from([("+79991234567".into(), "Черновик ❤️".into())]),
            selected: Some("+79991234567".into()),
            submitted: Some(("+79991234567".into(), "Незавершённая отправка".into())),
            ..Default::default()
        };
        d.save(&path).unwrap();
        assert!(!String::from_utf8_lossy(&std::fs::read(&path).unwrap()).contains("Черновик"));
        let restored = Drafts::load(&path).unwrap();
        assert_eq!(restored.messages, d.messages);
        assert_eq!(restored.submitted, d.submitted);
        std::fs::remove_file(config).unwrap();
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}
