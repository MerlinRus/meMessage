use crate::transport::{Exchange, Transport};
use std::io;

pub struct MapClient<T: Exchange> {
    transport: T,
    connection: Vec<u8>,
    peer_max: usize,
}

pub fn headers(packet: &[u8], mut offset: usize) -> io::Result<Vec<(u8, Vec<u8>)>> {
    let mut result = Vec::new();
    while offset < packet.len() {
        let id = packet[offset];
        let (prefix, length) = match id & 0xc0 {
            0 | 0x40 => {
                if offset + 3 > packet.len() {
                    return Err(io::Error::other("Truncated OBEX header"));
                }
                (
                    3,
                    u16::from_be_bytes([packet[offset + 1], packet[offset + 2]]) as usize,
                )
            }
            0x80 => (1, 2),
            _ => (1, 5),
        };
        if length < prefix || offset + length > packet.len() {
            return Err(io::Error::other("Invalid OBEX header size"));
        }
        result.push((id, packet[offset + prefix..offset + length].to_vec()));
        offset += length;
    }
    Ok(result)
}
fn header(id: u8, body: &[u8]) -> Vec<u8> {
    let size = (body.len() + 3) as u16;
    let mut p = vec![id, (size >> 8) as u8, size as u8];
    p.extend_from_slice(body);
    p
}
fn name(value: &str) -> Vec<u8> {
    header(
        1,
        &value
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_be_bytes)
            .collect::<Vec<_>>(),
    )
}
fn kind(value: &str) -> Vec<u8> {
    header(0x42, &[value.as_bytes(), &[0]].concat())
}
fn accepted(response: &[u8]) -> io::Result<()> {
    match response.first() {
        Some(0xa0) => Ok(()),
        Some(0xc3) => Err(io::Error::other(
            "iPhone запретил доступ. В Bluetooth на телефоне включи «Показывать уведомления» рядом с ноутбуком.",
        )),
        code => Err(io::Error::other(format!(
            "iPhone отклонил запрос ({code:02X?})."
        ))),
    }
}
impl MapClient<Transport> {
    pub fn connect(address: u64) -> io::Result<Self> {
        Self::open(Transport::connect(address)?)
    }
}
impl<T: Exchange> MapClient<T> {
    pub fn open(mut transport: T) -> io::Result<Self> {
        let response = transport.exchange(&[
            0x80, 0, 0x1a, 0x10, 0, 0x10, 0, 0x46, 0, 0x13, 0xbb, 0x58, 0x2b, 0x40, 0x42, 0x0c,
            0x11, 0xdb, 0xb0, 0xde, 0x08, 0, 0x20, 0x0c, 0x9a, 0x66,
        ])?;
        accepted(&response)?;
        if response.len() < 7 {
            return Err(io::Error::other("Incomplete OBEX CONNECT response"));
        }
        let peer_max = (u16::from_be_bytes([response[5], response[6]]) as usize).min(4096);
        if peer_max < 255 {
            return Err(io::Error::other("Invalid OBEX packet limit"));
        }
        let mut connection = Vec::new();
        for (id, bytes) in headers(&response, 7)? {
            if id == 0xcb {
                connection = vec![id];
                connection.extend(bytes);
            }
        }
        let mut client = Self {
            transport,
            connection,
            peer_max,
        };
        for path in ["", "telecom", "msg"] {
            let payload = [&[2, 0][..], &client.connection, &name(path)].concat();
            accepted(&client.transport.exchange(&header(0x85, &payload))?)?;
        }
        Ok(client)
    }
    fn get(&mut self, first: Vec<u8>) -> io::Result<Vec<u8>> {
        let mut body = Vec::new();
        for page in 0..512 {
            let response = self.transport.exchange(&header(
                0x83,
                if page == 0 { &first } else { &self.connection },
            ))?;
            if !matches!(response.first(), Some(0x90 | 0xa0)) {
                accepted(&response)?;
            }
            for (id, bytes) in headers(&response, 3)? {
                if id == 0x48 || id == 0x49 {
                    body.extend(bytes);
                }
            }
            if body.len() > 2 * 1024 * 1024 {
                return Err(io::Error::other("Message size limit exceeded"));
            }
            if response.first() == Some(&0xa0) {
                return Ok(body);
            }
        }
        Err(io::Error::other("Too many OBEX fragments"))
    }
    pub fn list(&mut self, folder: &str) -> io::Result<String> {
        self.list_page(folder, 0, 100)
    }
    pub fn list_page(&mut self, folder: &str, offset: u16, count: u16) -> io::Result<String> {
        if !["inbox", "sent"].contains(&folder) {
            return Err(io::Error::other("Unsupported folder"));
        }
        let request = [
            self.connection.clone(),
            name(folder),
            kind("x-bt/MAP-msg-listing"),
            header(
                0x4c,
                &[
                    1,
                    2,
                    (count >> 8) as u8,
                    count as u8,
                    2,
                    2,
                    (offset >> 8) as u8,
                    offset as u8,
                ],
            ),
        ]
        .concat();
        String::from_utf8(self.get(request)?).map_err(io::Error::other)
    }
    pub fn read(&mut self, handle: &str) -> io::Result<String> {
        let envelope = self.read_envelope(handle, false)?;
        extract_text(&envelope)
    }
    pub fn read_envelope(&mut self, handle: &str, attachments: bool) -> io::Result<String> {
        if handle.is_empty() || handle.len() > 32 || !handle.chars().all(|c| c.is_ascii_hexdigit())
        {
            return Err(io::Error::other("Invalid message handle"));
        }
        let request = [
            self.connection.clone(),
            name(handle),
            kind("x-bt/message"),
            header(0x4c, &[0x0a, 1, u8::from(attachments), 0x14, 1, 1]),
        ]
        .concat();
        String::from_utf8(self.get(request)?).map_err(io::Error::other)
    }
    pub fn send(&mut self, recipient: &str, text: &str) -> io::Result<String> {
        let body = bmessage(recipient, text)?;
        let first = [
            self.connection.clone(),
            name("outbox"),
            kind("x-bt/message"),
            header(0x4c, &[0x0b, 1, 0, 0x0c, 1, 0, 0x14, 1, 1]),
        ]
        .concat();
        let mut offset = 0;
        while offset < body.len() {
            let head = if offset == 0 {
                &first
            } else {
                &self.connection
            };
            let available = self
                .peer_max
                .checked_sub(head.len() + 6)
                .ok_or_else(|| io::Error::other("OBEX packet too small"))?;
            let length = available.min(body.len() - offset);
            if length == 0 {
                return Err(io::Error::other("Empty upload fragment"));
            }
            let final_packet = offset + length == body.len();
            let payload = [
                head.as_slice(),
                &header(
                    if final_packet { 0x49 } else { 0x48 },
                    &body[offset..offset + length],
                ),
            ]
            .concat();
            let response = self
                .transport
                .exchange(&header(if final_packet { 0x82 } else { 0x02 }, &payload))?;
            if final_packet {
                accepted(&response)?;
                for (id, bytes) in headers(&response, 3)? {
                    if id == 1 {
                        let chars: Vec<u16> = bytes
                            .as_chunks::<2>()
                            .0
                            .iter()
                            .map(|b| u16::from_be_bytes([b[0], b[1]]))
                            .collect();
                        return Ok(String::from_utf16_lossy(&chars)
                            .trim_end_matches('\0')
                            .to_owned());
                    }
                }
                return Ok(String::new());
            }
            if response.first() != Some(&0x90) {
                return Err(io::Error::other(
                    "Передача прервана. Проверь сообщение на iPhone перед повтором.",
                ));
            }
            offset += length;
        }
        Err(io::Error::other("Empty upload"))
    }
}
pub fn normalize_phone(value: &str) -> io::Result<String> {
    let clean: String = value
        .chars()
        .filter(|c| !c.is_whitespace() && !['(', ')', '-'].contains(c))
        .collect();
    let digits = clean.strip_prefix('+').unwrap_or(&clean);
    if !(7..=15).contains(&digits.len()) || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(io::Error::other(
            "Укажи номер телефона: от 7 до 15 цифр, можно с + в начале.",
        ));
    }
    Ok(clean)
}
fn escape_lines(text: &str, encode: bool) -> String {
    text.split("\r\n")
        .map(|line| {
            let tail = line.trim_start_matches('/');
            if tail.starts_with("END:MSG") {
                if encode {
                    format!("/{line}")
                } else {
                    line.strip_prefix('/').unwrap_or(line).to_owned()
                }
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\r\n")
}
pub fn bmessage(recipient: &str, text: &str) -> io::Result<Vec<u8>> {
    let phone = normalize_phone(recipient)?;
    let text = text
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\n', "\r\n");
    if text.trim().is_empty() || text.contains('\0') || text.len() > 12000 {
        return Err(io::Error::other("Введи сообщение не длиннее 12 КБ."));
    }
    let framed = format!("BEGIN:MSG\r\n{}\r\nEND:MSG\r\n", escape_lines(&text, true));
    Ok(format!("BEGIN:BMSG\r\nVERSION:1.0\r\nSTATUS:UNREAD\r\nTYPE:SMS_GSM\r\nFOLDER:telecom/msg/outbox\r\nBEGIN:BENV\r\nBEGIN:VCARD\r\nVERSION:2.1\r\nN:\r\nTEL:{phone}\r\nEND:VCARD\r\nBEGIN:BBODY\r\nCHARSET:UTF-8\r\nLENGTH:{}\r\n{framed}END:BBODY\r\nEND:BENV\r\nEND:BMSG\r\n",framed.len()).into_bytes())
}
pub fn extract_text(envelope: &str) -> io::Result<String> {
    let start = envelope
        .find("BEGIN:MSG\r\n")
        .map(|n| n + 11)
        .ok_or_else(|| io::Error::other("Message has no text body"))?;
    let end = envelope
        .rfind("\r\nEND:MSG")
        .filter(|&n| n >= start)
        .ok_or_else(|| io::Error::other("Invalid message body"))?;
    Ok(escape_lines(&envelope[start..end], false))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_and_delimiter_roundtrip() {
        let text = "Привет 👋\nEND:MSG\n/END:MSG\nГотово";
        let envelope = String::from_utf8(bmessage("+7 (999) 123-45-67", text).unwrap()).unwrap();
        assert!(envelope.contains("TEL:+79991234567\r\n"));
        assert_eq!(extract_text(&envelope).unwrap(), text.replace('\n', "\r\n"));
        let declared: usize = envelope
            .split("LENGTH:")
            .nth(1)
            .unwrap()
            .split("\r\n")
            .next()
            .unwrap()
            .parse()
            .unwrap();
        let first = envelope.find("BEGIN:MSG\r\n").unwrap();
        let last = envelope.rfind("END:MSG\r\n").unwrap() + 9;
        assert_eq!(declared, envelope[first..last].len());
    }
    #[test]
    fn rejects_injection_and_malformed_packets() {
        for value in ["+7999\r\nEND:VCARD", "abc", "+123", "test@example.com"] {
            assert!(bmessage(value, "test").is_err());
        }
        for packet in [
            &[0xa0, 0, 5, 0x49, 0][..],
            &[0xa0, 0, 6, 0x49, 0, 2],
            &[0xa0, 0, 6, 0x49, 0, 9],
            &[0xa0, 0, 4, 0xcb],
        ] {
            assert!(headers(packet, 3).is_err());
        }
    }
    #[test]
    fn fragmented_upload_is_not_retried() {
        struct Fake {
            packets: Vec<Vec<u8>>,
        }
        impl Exchange for Fake {
            fn exchange(&mut self, packet: &[u8]) -> io::Result<Vec<u8>> {
                self.packets.push(packet.to_vec());
                Ok(vec![if packet[0] == 0x82 { 0xa0 } else { 0x90 }, 0, 3])
            }
        }
        let mut client = MapClient {
            transport: Fake { packets: vec![] },
            connection: vec![0xcb, 0, 0, 0, 1],
            peer_max: 255,
        };
        client.send("+79991234567", &"я".repeat(1000)).unwrap();
        assert!(client.transport.packets.len() > 5);
        assert!(client.transport.packets.iter().all(|p| p.len() <= 255));
        assert_eq!(client.transport.packets.last().unwrap()[0], 0x82);
        let mut collected = Vec::new();
        for packet in &client.transport.packets {
            for (id, body) in headers(packet, 3).unwrap() {
                if id == 0x48 || id == 0x49 {
                    collected.extend(body);
                }
            }
        }
        assert_eq!(
            collected,
            bmessage("+79991234567", &"я".repeat(1000)).unwrap()
        );
    }
}
