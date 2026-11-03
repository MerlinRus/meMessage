use crate::{
    map, model,
    transport::{Exchange, Transport},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io,
    path::{Path, PathBuf},
};

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Book {
    #[serde(default)]
    pub imported: BTreeMap<String, String>,
    #[serde(default)]
    pub aliases: BTreeMap<String, String>,
    #[serde(default)]
    pub photos: BTreeMap<String, Vec<u8>>,
}
pub fn path() -> PathBuf {
    model::data_path().with_file_name("contacts.rust.bin")
}
pub fn meaningful(name: &str, phone: &str) -> bool {
    !name.trim().is_empty() && model::key(name) != model::key(phone)
}
impl Book {
    pub fn load(path: &Path) -> io::Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        serde_json::from_slice(&model::protect(&std::fs::read(path)?, false)?)
            .map_err(io::Error::other)
    }
    pub fn save(&self, path: &Path) -> io::Result<()> {
        let bytes = model::protect(&serde_json::to_vec(self)?, true)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let temp = path.with_extension("tmp");
        std::fs::write(&temp, bytes)?;
        std::fs::rename(temp, path)
    }
    pub fn display(&self, phone: &str, provided: &str) -> String {
        let key = model::key(phone);
        self.aliases
            .get(&key)
            .or_else(|| self.imported.get(&key))
            .cloned()
            .or_else(|| meaningful(provided, phone).then(|| provided.trim().to_owned()))
            .unwrap_or_else(|| model::format_phone(phone))
    }
    pub fn set_alias(&mut self, phone: &str, name: &str) -> io::Result<()> {
        let name = name.trim();
        if name.chars().count() > 100 || name.chars().any(char::is_control) {
            return Err(io::Error::other(
                "Имя должно быть короче 100 символов и без переносов строк.",
            ));
        }
        let key = model::key(phone);
        if name.is_empty() {
            self.aliases.remove(&key);
        } else {
            self.aliases.insert(key, name.into());
        }
        Ok(())
    }
    pub fn apply(&self, conversations: &mut [model::Conversation]) {
        for c in conversations {
            c.name = self.display(&c.phone, &c.name);
        }
    }
}

fn variable_header(id: u8, value: &[u8]) -> Vec<u8> {
    let len = (value.len() + 3) as u16;
    [&[id, (len >> 8) as u8, len as u8], value].concat()
}
pub fn pull(address: u64) -> io::Result<BTreeMap<String, String>> {
    let book = pull_with(Transport::connect_service(address, 0x112f)?)?;
    if book.is_empty() {
        Err(io::Error::other(
            "iPhone вернул пустую телефонную книгу. Включи «Синхронизация контактов» в Bluetooth на телефоне и повтори.",
        ))
    } else {
        Ok(book)
    }
}
fn pull_with(transport: impl Exchange) -> io::Result<BTreeMap<String, String>> {
    Ok(parse_vcards(&pull_raw(transport)?))
}
pub fn probe(address: u64) -> io::Result<serde_json::Value> {
    let text = pull_raw(Transport::connect_service(address, 0x112f)?)?;
    let numbers = parse_vcards(&text);
    let cached = model::load(&model::data_path()).unwrap_or_default();
    let phones: std::collections::BTreeSet<_> = cached
        .iter()
        .map(|message| model::key(&message.phone))
        .collect();
    let matched = phones
        .iter()
        .filter(|phone| numbers.contains_key(*phone))
        .count();
    Ok(
        serde_json::json!({"bytes":text.len(),"cards":text.matches("BEGIN:VCARD").count(),"numbers":numbers.len(),"matched_conversations":matched}),
    )
}
fn pull_raw(mut transport: impl Exchange) -> io::Result<String> {
    let response = transport.exchange(&[
        0x80, 0, 0x1a, 0x10, 0, 0x10, 0, 0x46, 0, 0x13, 0x79, 0x61, 0x35, 0xf0, 0xf0, 0xc5, 0x11,
        0xd8, 0x09, 0x66, 0x08, 0, 0x20, 0x0c, 0x9a, 0x66,
    ])?;
    check(&response)?;
    if response.len() < 7 {
        return Err(io::Error::other("Неполный ответ телефонной книги iPhone."));
    }
    let mut connection = Vec::new();
    for (id, bytes) in map::headers(&response, 7)? {
        if id == 0xcb {
            connection.push(id);
            connection.extend(bytes);
        }
    }
    let name: Vec<_> = "telecom/pb.vcf"
        .encode_utf16()
        .chain(Some(0))
        .flat_map(u16::to_be_bytes)
        .collect();
    // Request names and telephone numbers only; do not transfer contact photos.
    let params = [6, 8, 0, 0, 0, 0, 0, 0, 0, 0x86, 7, 1, 1, 4, 2, 0x27, 0x10];
    let first = [
        connection.clone(),
        variable_header(1, &name),
        variable_header(0x42, b"x-bt/phonebook\0"),
        variable_header(0x4c, &params),
    ]
    .concat();
    let mut body = Vec::new();
    for page in 0..2048 {
        let reply = transport.exchange(&variable_header(
            0x83,
            if page == 0 { &first } else { &connection },
        ))?;
        if !matches!(reply.first(), Some(0x90 | 0xa0)) {
            check(&reply)?;
        }
        for (id, bytes) in map::headers(&reply, 3)? {
            if id == 0x48 || id == 0x49 {
                body.extend(bytes);
            }
        }
        if body.len() > 8 * 1024 * 1024 {
            return Err(io::Error::other("Телефонная книга превышает 8 МБ."));
        }
        if reply.first() == Some(&0xa0) {
            return String::from_utf8(body).map_err(io::Error::other);
        }
    }
    Err(io::Error::other("Слишком много частей телефонной книги."))
}
fn check(reply: &[u8]) -> io::Result<()> {
    match reply.first() {
        Some(0xa0) => Ok(()),
        Some(0xc3) => Err(io::Error::other(
            "На iPhone включи «Синхронизация контактов» в Bluetooth → ⓘ рядом с ноутбуком, затем повтори.",
        )),
        _ => Err(io::Error::other(
            "iPhone не предоставил телефонную книгу по Bluetooth.",
        )),
    }
}
fn unescape(value: &str) -> String {
    let mut out = String::new();
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(next) = chars.next() {
                out.push(if next == 'n' || next == 'N' {
                    ' '
                } else {
                    next
                });
            }
        } else {
            out.push(c);
        }
    }
    out
}
fn quoted_printable(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'='
            && index + 2 < bytes.len()
            && let Some(byte) = std::str::from_utf8(&bytes[index + 1..index + 3])
                .ok()
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            out.push(byte);
            index += 3;
            continue;
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
pub fn parse_vcards(text: &str) -> BTreeMap<String, String> {
    let mut lines: Vec<String> = Vec::new();
    for line in text.lines() {
        if let Some(last) = lines.last_mut() {
            if last.contains("ENCODING=QUOTED-PRINTABLE") && last.ends_with('=') {
                last.pop();
                last.push_str(line.trim_start());
                continue;
            }
            if line.starts_with([' ', '\t']) {
                last.push_str(&line[1..]);
                continue;
            }
        }
        lines.push(line.trim_end_matches('\r').into());
    }
    let mut result = BTreeMap::new();
    let (mut full, mut parts, mut phones) = (String::new(), String::new(), Vec::<String>::new());
    for line in lines {
        if line.eq_ignore_ascii_case("BEGIN:VCARD") {
            full.clear();
            parts.clear();
            phones.clear();
            continue;
        }
        if line.eq_ignore_ascii_case("END:VCARD") {
            let name = if full.trim().is_empty() {
                parts.trim()
            } else {
                full.trim()
            };
            if !name.is_empty() {
                for phone in &phones {
                    result.insert(phone.clone(), name.to_owned());
                }
            }
            continue;
        }
        let Some((field, value)) = line.split_once(':') else {
            continue;
        };
        let field = field.to_uppercase();
        let property = field
            .split(';')
            .next()
            .unwrap_or("")
            .rsplit('.')
            .next()
            .unwrap_or("");
        let value = if field.contains("ENCODING=QUOTED-PRINTABLE") {
            quoted_printable(value)
        } else {
            value.into()
        };
        match property {
            "FN" => full = unescape(&value),
            "N" => {
                parts = value
                    .split(';')
                    .map(unescape)
                    .filter(|s| !s.trim().is_empty())
                    .collect::<Vec<_>>()
                    .join(" ")
            }
            "TEL" => {
                let phone = model::key(&value);
                if (8..=16).contains(&phone.len())
                    && phone
                        .strip_prefix('+')
                        .is_some_and(|digits| digits.bytes().all(|b| b.is_ascii_digit()))
                {
                    phones.push(phone);
                }
            }
            _ => {}
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names_unicode_multiple_numbers_and_escaped_punctuation() {
        let book = parse_vcards(
            "BEGIN:VCARD\r\nFN:Полина ❤️\\, любимая\r\nTEL;CELL:+7 (999) 123-45-67\r\nitem1.TEL:89997654321\r\nEND:VCARD\r\nBEGIN:VCARD\r\nN:Иванов;Иван;;;\r\nTEL:+79990000000\r\nEND:VCARD",
        );
        assert_eq!(book.len(), 3);
        assert_eq!(book["+79991234567"], "Полина ❤️, любимая");
        assert_eq!(book["+79990000000"], "Иванов Иван");
    }
    #[test]
    fn manual_names_override_import_and_survive_encrypted_reload() {
        let mut book = Book::default();
        book.imported.insert("+79991234567".into(), "Полина".into());
        book.set_alias("8 (999) 123-45-67", "Любимая ❤️").unwrap();
        assert_eq!(book.display("+79991234567", "+79991234567"), "Любимая ❤️");
        let path = std::env::temp_dir().join(format!("bridge-contacts-{}.bin", std::process::id()));
        book.save(&path).unwrap();
        assert!(!String::from_utf8_lossy(&std::fs::read(&path).unwrap()).contains("Любимая"));
        let mut loaded = Book::load(&path).unwrap();
        loaded.set_alias("+79991234567", "").unwrap();
        assert_eq!(loaded.display("+79991234567", ""), "Полина");
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn phone_numbers_are_not_contact_names() {
        assert!(!meaningful("+7 999 123-45-67", "89991234567"));
        assert!(meaningful("Полина ❤️", "+79991234567"));
    }
    #[test]
    fn local_photos_and_legacy_books_survive_private_reload() {
        let old: Book = serde_json::from_str(r#"{"imported":{},"aliases":{}}"#).unwrap();
        assert!(old.photos.is_empty());
        let mut book = old;
        let mut image = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(8, 8)
            .write_to(&mut image, image::ImageFormat::Png)
            .unwrap();
        let photo = crate::avatars::prepare(image.get_ref()).unwrap();
        book.photos.insert("+79991234567".into(), photo.clone());
        book.set_alias("+79991234567", "Имя с фото").unwrap();
        let path =
            std::env::temp_dir().join(format!("meMessage-photo-test-{}.bin", std::process::id()));
        book.save(&path).unwrap();
        let raw = std::fs::read(&path).unwrap();
        assert!(!raw.windows(8).any(|part| part == b"\x89PNG\r\n\x1a\n"));
        let mut loaded = Book::load(&path).unwrap();
        loaded
            .imported
            .insert("+79991234567".into(), "Имя iPhone".into());
        loaded.save(&path).unwrap();
        let loaded = Book::load(&path).unwrap();
        assert_eq!(loaded.photos["+79991234567"], photo);
        assert_eq!(loaded.display("+79991234567", ""), "Имя с фото");
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn pbap_requests_names_without_photos_and_combines_fragments() {
        struct Fake {
            step: usize,
        }
        impl Exchange for Fake {
            fn exchange(&mut self, p: &[u8]) -> io::Result<Vec<u8>> {
                self.step += 1;
                match self.step {
                    1 => {
                        assert_eq!(p[10], 0x79);
                        Ok(vec![0xa0, 0, 7, 0x10, 0, 0x10, 0])
                    }
                    2 => {
                        assert!(p.windows(4).any(|w| w == [6, 8, 0, 0]));
                        Ok(variable_header(
                            0x90,
                            &variable_header(0x48, b"BEGIN:VCARD\r\nFN:Anna\r\n"),
                        ))
                    }
                    _ => Ok(variable_header(
                        0xa0,
                        &variable_header(0x49, b"TEL:+79991234567\r\nEND:VCARD\r\n"),
                    )),
                }
            }
        }
        let book = pull_with(Fake { step: 0 }).unwrap();
        assert_eq!(book["+79991234567"], "Anna");
    }
}
