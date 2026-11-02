use chrono::{Local, TimeZone};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
    ptr,
};
use windows_sys::Win32::{Foundation::LocalFree, Security::Cryptography::*};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum SendState {
    Received,
    Pending,
    Accepted,
    Unconfirmed,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub handle: String,
    pub phone: String,
    pub name: String,
    pub text: String,
    pub timestamp: i64,
    pub outgoing: bool,
    pub read: bool,
    pub state: SendState,
}
#[derive(Clone, Debug)]
pub struct Conversation {
    pub phone: String,
    pub name: String,
    pub preview: String,
    pub time: i64,
    pub unread: bool,
}
pub fn key(value: &str) -> String {
    let value = value.trim();
    if value.contains('@') {
        return value.to_lowercase();
    }
    let mut digits: String = value.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() == 11 && digits.starts_with('8') {
        digits.replace_range(..1, "7");
    }
    if digits.is_empty() {
        value.to_owned()
    } else {
        format!("+{digits}")
    }
}
pub fn format_phone(phone: &str) -> String {
    let value = key(phone);
    if value.starts_with("+7") && value.len() == 12 {
        format!(
            "+7 {} {}-{}-{}",
            &value[2..5],
            &value[5..8],
            &value[8..10],
            &value[10..]
        )
    } else {
        value
    }
}
pub fn conversations(messages: &[Message]) -> Vec<Conversation> {
    let mut groups: BTreeMap<String, Vec<&Message>> = BTreeMap::new();
    for m in messages {
        groups.entry(key(&m.phone)).or_default().push(m);
    }
    let mut result = Vec::new();
    for (phone, mut records) in groups {
        records.sort_by_key(|m| m.timestamp);
        let last = records.last().unwrap();
        let name = records
            .iter()
            .rev()
            .find(|m| crate::contacts::meaningful(&m.name, &phone))
            .map(|m| m.name.clone())
            .unwrap_or_else(|| format_phone(&phone));
        result.push(Conversation {
            phone,
            name,
            preview: format!(
                "{}{}",
                if last.outgoing { "Вы: " } else { "" },
                if crate::media::is_photo_placeholder(&last.text) {
                    "Фото".into()
                } else {
                    last.text.replace(['\r', '\n'], " ")
                }
            ),
            time: last.timestamp,
            unread: records.iter().any(|m| !m.outgoing && !m.read),
        });
    }
    result.sort_by_key(|c| std::cmp::Reverse(c.time));
    result
}
pub fn time_label(timestamp: i64) -> String {
    let Some(time) = Local.timestamp_opt(timestamp, 0).single() else {
        return String::new();
    };
    if time.date_naive() == Local::now().date_naive() {
        time.format("%H:%M").to_string()
    } else {
        time.format("%d.%m").to_string()
    }
}
pub fn data_path() -> PathBuf {
    let exe = std::env::current_exe().unwrap_or_default();
    let folder = exe.parent().unwrap_or(Path::new("."));
    let root = folder
        .ancestors()
        .find(|p| p.join("Cargo.toml").exists())
        .unwrap_or(folder);
    root.join("data").join("history.rust.bin")
}
pub(crate) fn protect(data: &[u8], encrypt: bool) -> io::Result<Vec<u8>> {
    let input = CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
        pbData: data.as_ptr() as *mut u8,
    };
    let entropy = b"iPhoneBridge.rust.v1";
    let entropy = CRYPT_INTEGER_BLOB {
        cbData: entropy.len() as u32,
        pbData: entropy.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    // UI_FORBIDDEN prevents Windows from showing credential/permission dialogs.
    let result = unsafe {
        if encrypt {
            CryptProtectData(
                &input,
                ptr::null(),
                &entropy,
                ptr::null(),
                ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                ptr::null_mut(),
                &entropy,
                ptr::null(),
                ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
    };
    if result == 0 {
        return Err(io::Error::last_os_error());
    }
    let bytes =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    unsafe {
        LocalFree(output.pbData.cast());
    }
    Ok(bytes)
}
pub fn load(path: &Path) -> io::Result<Vec<Message>> {
    if !path.exists() {
        return Ok(vec![]);
    }
    let bytes = protect(&fs::read(path)?, false)?;
    let mut records: Vec<Message> = serde_json::from_slice(&bytes)?;
    for m in &mut records {
        if m.state == SendState::Pending {
            m.state = SendState::Unconfirmed;
        }
    }
    Ok(records)
}
pub fn save(path: &Path, messages: &[Message]) -> io::Result<()> {
    let mut records = messages.to_vec();
    records.sort_by_key(|m| std::cmp::Reverse(m.timestamp));
    records.truncate(2000);
    let bytes = protect(&serde_json::to_vec(&records)?, true)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension("tmp");
    fs::write(&temp, bytes)?;
    fs::rename(temp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn phone_keys_and_conversation_order() {
        assert_eq!(key("8 (999) 123-45-67"), key("+7 999 1234567"));
        let m = |id: &str, phone: &str, time| Message {
            id: id.into(),
            handle: String::new(),
            phone: phone.into(),
            name: String::new(),
            text: "Текст".into(),
            timestamp: time,
            outgoing: false,
            read: true,
            state: SendState::Received,
        };
        let groups = conversations(&[
            m("1", "+79991234567", 10),
            m("2", "89991234567", 30),
            m("3", "+79990000000", 20),
        ]);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].time, 30);
    }
    #[test]
    fn encrypted_unicode_history_roundtrip() {
        let path =
            std::env::temp_dir().join(format!("iphone-bridge-test-{}.bin", std::process::id()));
        let message = Message {
            id: "test".into(),
            handle: String::new(),
            phone: "+79990000000".into(),
            name: "Анна".into(),
            text: "Привет 👋".into(),
            timestamp: 123,
            outgoing: true,
            read: true,
            state: SendState::Pending,
        };
        save(&path, &[message]).unwrap();
        assert!(!String::from_utf8_lossy(&fs::read(&path).unwrap()).contains("Привет"));
        let loaded = load(&path).unwrap();
        assert_eq!(loaded[0].text, "Привет 👋");
        assert_eq!(loaded[0].state, SendState::Unconfirmed);
        fs::remove_file(path).unwrap();
    }
}
