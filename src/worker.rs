use crate::{
    contacts::{self, Book},
    map::MapClient,
    model::{self, Message, SendState},
    notifications::{NotificationTracker, Notifications},
    transport,
};
use chrono::{Local, NaiveDateTime, TimeZone};
use eframe::egui;
use serde::Deserialize;
use std::{
    collections::HashSet,
    io,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::{Duration, SystemTime},
};

pub enum Command {
    Connect,
    MarkRead(Vec<String>),
    Refresh,
    SyncHistory,
    SyncContacts,
    Send {
        phone: String,
        target: String,
        text: String,
    },
    Stop,
}
pub enum Event {
    Connecting,
    Snapshot(Vec<Message>),
    Connection(bool, String),
    ReconnectIn(u64),
    Sent(String),
    SendFailed(String),
    OpenConversation(String),
    NotificationError(String),
    Contacts(Book),
    ContactsStatus(bool, String),
    StorageError(String),
}

struct PollSchedule {
    failures: u32,
    next: SystemTime,
}

enum Submission {
    Accepted(String),
    Failed(String),
    Unconfirmed,
}
fn submit_once<T: transport::Exchange>(
    session: io::Result<MapClient<T>>,
    target: &str,
    text: &str,
) -> Submission {
    if let Err(error) = crate::map::bmessage(target, text) {
        return Submission::Failed(error.to_string());
    }
    match session {
        Err(error) => Submission::Failed(error.to_string()),
        Ok(mut session) => match session.send(target, text) {
            Ok(handle) => Submission::Accepted(handle),
            Err(_) => Submission::Unconfirmed,
        },
    }
}
impl PollSchedule {
    fn new() -> Self {
        Self {
            failures: 0,
            next: SystemTime::now(),
        }
    }
    fn remaining(&self) -> Duration {
        self.next
            .duration_since(SystemTime::now())
            .unwrap_or_default()
    }
    fn completed(&mut self, connected: bool, period: u64) -> u64 {
        self.failures = if connected {
            0
        } else {
            self.failures.saturating_add(1)
        };
        let seconds = if connected {
            period.clamp(2, 60)
        } else {
            (3u64 << self.failures.min(5).saturating_sub(1)).min(30)
        };
        self.next = SystemTime::now() + Duration::from_secs(seconds);
        seconds
    }
}

#[derive(Deserialize, Default)]
struct Listing {
    #[serde(rename = "msg", default)]
    entries: Vec<Entry>,
}
#[derive(Deserialize)]
struct Entry {
    #[serde(rename = "@handle", default)]
    handle: String,
    #[serde(rename = "@sender_addressing", default)]
    sender: String,
    #[serde(rename = "@recipient_addressing", default)]
    recipient: String,
    #[serde(rename = "@sender_name", default)]
    sender_name: String,
    #[serde(rename = "@recipient_name", default)]
    recipient_name: String,
    #[serde(rename = "@subject", default)]
    subject: String,
    #[serde(rename = "@datetime", default)]
    datetime: String,
    #[serde(rename = "@read", default)]
    read: String,
}
fn listing(xml: &str, outgoing: bool) -> io::Result<Vec<Message>> {
    let listing: Listing = quick_xml::de::from_str(xml).map_err(io::Error::other)?;
    Ok(listing
        .entries
        .into_iter()
        .filter_map(|e| {
            let phone = if outgoing { e.recipient } else { e.sender };
            if phone.trim().is_empty() {
                return None;
            }
            let timestamp = NaiveDateTime::parse_from_str(&e.datetime, "%Y%m%dT%H%M%S")
                .ok()
                .and_then(|t| Local.from_local_datetime(&t).single())
                .map(|t| t.timestamp())
                .unwrap_or_else(|| Local::now().timestamp());
            Some(Message {
                id: format!("{}:{}", if outgoing { "sent" } else { "in" }, e.handle),
                handle: e.handle,
                phone: model::key(&phone),
                name: if outgoing {
                    e.recipient_name
                } else {
                    e.sender_name
                },
                text: e.subject,
                timestamp,
                outgoing,
                read: e.read == "yes",
                state: if outgoing {
                    SendState::Accepted
                } else {
                    SendState::Received
                },
            })
        })
        .collect())
}
pub fn refresh(cache: &mut Vec<Message>) -> io::Result<()> {
    refresh_history(cache, false)
}
fn refresh_history(cache: &mut Vec<Message>, history: bool) -> io::Result<()> {
    let address = transport::find_iphone()?;
    let mut session = MapClient::connect(address)?;
    let mut records = listing(&session.list("inbox")?, false)?;
    if let Ok(xml) = session.list("sent")
        && let Ok(sent) = listing(&xml, true)
    {
        records.extend(sent);
    }
    if history {
        for (folder, outgoing) in [("inbox", false), ("sent", true)] {
            let mut offset = records
                .iter()
                .filter(|message| message.outgoing == outgoing)
                .count() as u16;
            let mut ids: HashSet<_> = records.iter().map(|message| message.id.clone()).collect();
            while offset > 0 {
                let Ok(xml) = session.list_page(folder, offset, 100) else {
                    break;
                };
                let page = listing(&xml, outgoing)?;
                let count = page.len() as u16;
                let mut added = 0;
                for message in page {
                    if ids.insert(message.id.clone()) {
                        records.push(message);
                        added += 1;
                    }
                }
                if added == 0 {
                    break;
                }
                let Some(next) = offset.checked_add(count) else {
                    break;
                };
                offset = next;
            }
        }
    }
    let known: HashSet<String> = cache.iter().map(|m| m.id.clone()).collect();
    for mut message in records {
        if let Some(existing) = cache.iter_mut().find(|m| m.id == message.id) {
            existing.read |= message.read;
            if contacts::meaningful(&message.name, &message.phone) {
                existing.name = message.name;
            }
            continue;
        }
        if !known.contains(&message.id) {
            if let Ok(text) = session.read(&message.handle) {
                message.text = text;
            }
            cache.push(message);
        }
    }
    cache.sort_by_key(|m| m.timestamp);
    Ok(())
}
pub fn history_probe() -> io::Result<serde_json::Value> {
    let mut session = MapClient::connect(transport::find_iphone()?)?;
    let mut seen = HashSet::new();
    let mut pages = Vec::new();
    for offset in [0, 10, 20, 30] {
        let xml = session.list_page("inbox", offset, 10)?;
        let entries = listing(&xml, false)?;
        let new = entries
            .iter()
            .filter(|entry| seen.insert(entry.id.clone()))
            .count();
        pages.push(serde_json::json!({"offset":offset,"records":entries.len(),"new_records":new}));
    }
    Ok(serde_json::json!({"pages":pages,"distinct":seen.len()}))
}
pub fn attachment_probe() -> io::Result<serde_json::Value> {
    let mut session = MapClient::connect(transport::find_iphone()?)?;
    let xml = session.list("inbox")?;
    let listing: Listing = quick_xml::de::from_str(&xml).map_err(io::Error::other)?;
    let mut samples = Vec::new();
    for entry in listing.entries.iter().take(8) {
        let without = session.read_envelope(&entry.handle, false)?;
        let with = session.read_envelope(&entry.handle, true)?;
        let lower = with.to_lowercase();
        let name_is_number = model::key(&entry.sender_name) == model::key(&entry.sender);
        let body = crate::map::extract_text(&with).unwrap_or_default();
        let fields: Vec<_> = with
            .lines()
            .filter_map(|line| line.split_once(':'))
            .filter(|(key, _)| {
                [
                    "TYPE",
                    "CHARSET",
                    "ENCODING",
                    "LENGTH",
                    "CONTENT-TYPE",
                    "CONTENT-TRANSFER-ENCODING",
                ]
                .contains(&key.to_uppercase().as_str())
            })
            .map(|(key, value)| (key.to_owned(), value.to_owned()))
            .collect();
        samples.push(serde_json::json!({"bytes_without":without.len(),"bytes_with":with.len(),"same_response":without==with,"name_is_number":name_is_number,"listing_name":!entry.sender_name.trim().is_empty(),"envelope_name":with.lines().any(|line|line.strip_prefix("FN:").or_else(||line.strip_prefix("N:")).is_some_and(|name|!name.trim_matches(';').trim().is_empty())),"image_payload":lower.contains("content-type: image/")||lower.contains("content-type:image/"),"attachment_placeholder":body.to_lowercase().contains("attachment")&&body.to_lowercase().contains("image"),"fields":fields}));
    }
    Ok(serde_json::json!({"message_count":listing.entries.len(),"samples":samples}))
}
pub fn start(
    ctx: egui::Context,
    path: PathBuf,
    book: Arc<Mutex<Book>>,
    preferences: Arc<Mutex<crate::settings::Settings>>,
    active_conversation: Arc<Mutex<Option<String>>>,
) -> (Sender<Command>, Receiver<Event>) {
    let (commands, rx) = mpsc::channel();
    let (tx, events) = mpsc::channel();
    thread::spawn(move || {
        let emit = |event: Event| {
            let _ = tx.send(event);
            ctx.request_repaint();
        };
        let mut writable = true;
        let mut cache = match model::load(&path) {
            Ok(records) => records,
            Err(_) => {
                writable = false;
                emit(Event::StorageError("Сохранённая история не открылась. Исходный файл сохранён; новые сообщения временно остаются в памяти.".into()));
                vec![]
            }
        };
        emit(Event::Snapshot(cache.clone()));
        let mut saved_cache = cache.clone();
        let save = |cache: &Vec<Message>, saved: &mut Vec<Message>, enabled: bool| {
            if enabled && writable && cache != saved {
                match model::save(&path,cache){Ok(())=>*saved=cache.clone(),Err(_)=>emit(Event::StorageError("Не удалось сохранить историю. Проверь свободное место и доступ к папке данных.".into()))}
            }
        };
        let mut notification_tracker = NotificationTracker::default();
        let mut notifications = match Notifications::new() {
            Ok(notifications) => Some(notifications),
            Err(error) => {
                emit(Event::NotificationError(error));
                None
            }
        };
        let mut first = true;
        let mut initial_contacts = true;
        let mut was_connected = false;
        let mut schedule = PollSchedule::new();
        let mut baseline = true;
        let sync_contacts = || {
            emit(Event::ContactsStatus(
                true,
                "Синхронизация контактов…".into(),
            ));
            let result = transport::find_iphone()
                .and_then(contacts::pull)
                .and_then(|imported| {
                    let mut shared = book
                        .lock()
                        .map_err(|_| io::Error::other("Контакты заняты."))?;
                    let mut updated = shared.clone();
                    updated.imported = imported.imported;
                    updated.linked_addresses = imported.linked_addresses;
                    updated.save(&contacts::path())?;
                    *shared = updated.clone();
                    Ok(updated)
                });
            match result {
                Ok(book) => {
                    let count = book.imported.len();
                    emit(Event::Contacts(book));
                    emit(Event::ContactsStatus(
                        false,
                        format!("Адресов из контактов iPhone: {count}."),
                    ));
                }
                Err(error) => emit(Event::ContactsStatus(false, error.to_string())),
            }
        };
        loop {
            let command = if first {
                first = false;
                Command::Connect
            } else {
                match rx.recv_timeout(schedule.remaining().min(Duration::from_secs(1))) {
                    Ok(command) => command,
                    Err(mpsc::RecvTimeoutError::Timeout) if schedule.remaining().is_zero() => {
                        Command::Refresh
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(_) => break,
                }
            };
            // Preferences can change while recv_timeout waits for a command.
            let settings = preferences
                .lock()
                .map(|settings| settings.clone())
                .unwrap_or_default();
            match command {
                Command::Stop => break,
                Command::MarkRead(ids) => {
                    if model::mark_read(&mut cache, &ids) {
                        save(&cache, &mut saved_cache, settings.save_history);
                        emit(Event::Snapshot(cache.clone()));
                    }
                }
                Command::SyncContacts => sync_contacts(),
                Command::Connect | Command::Refresh | Command::SyncHistory => {
                    if !was_connected {
                        emit(Event::Connecting);
                    }
                    let historical = matches!(command, Command::SyncHistory)
                        || (!was_connected && settings.sync_history_on_connect);
                    match refresh_history(&mut cache, historical) {
                        Ok(()) => {
                            let restored = !was_connected && !baseline;
                            was_connected = true;
                            schedule.completed(true, settings.poll_seconds);
                            let current_settings =
                                preferences.lock().map(|s| s.clone()).unwrap_or_default();
                            let active = active_conversation
                                .lock()
                                .ok()
                                .and_then(|phone| phone.clone());
                            for message in notification_tracker.observe(&cache) {
                                if baseline
                                    || !crate::notifications::should_notify(
                                        message,
                                        &current_settings,
                                        active.as_deref(),
                                    )
                                {
                                    continue;
                                }
                                if let Some(notifications) = &mut notifications {
                                    let title = book
                                        .lock()
                                        .map(|book| book.display(&message.phone, &message.name))
                                        .unwrap_or_else(|_| model::format_phone(&message.phone));
                                    let phone = message.phone.clone();
                                    let events = tx.clone();
                                    let context = ctx.clone();
                                    let click = std::sync::Arc::new(move || {
                                        let _ = events.send(Event::OpenConversation(phone.clone()));
                                        context.request_repaint();
                                    });
                                    if let Err(error) = notifications.show_with_sound(
                                        &title,
                                        crate::notifications::preview_text(
                                            &message.text,
                                            current_settings.notification_preview,
                                        ),
                                        Some(click),
                                        current_settings.notification_sound,
                                    ) {
                                        emit(Event::NotificationError(error));
                                    }
                                }
                            }
                            baseline = false;
                            emit(Event::Snapshot(cache.clone()));
                            emit(Event::Connection(
                                true,
                                if restored {
                                    "Связь восстановлена"
                                } else {
                                    "iPhone подключён"
                                }
                                .into(),
                            ));
                            save(&cache, &mut saved_cache, settings.save_history);
                            if initial_contacts {
                                initial_contacts = false;
                                sync_contacts();
                            }
                        }
                        Err(error) => {
                            was_connected = false;
                            emit(Event::Connection(false, error.to_string()));
                            emit(Event::ReconnectIn(
                                schedule.completed(false, settings.poll_seconds),
                            ));
                        }
                    }
                }
                Command::Send {
                    phone,
                    target,
                    text,
                } => {
                    let id = format!(
                        "local:{}",
                        Local::now().timestamp_nanos_opt().unwrap_or_default()
                    );
                    let message = Message {
                        id: id.clone(),
                        handle: String::new(),
                        phone: phone.clone(),
                        name: cache
                            .iter()
                            .rev()
                            .find(|m| m.phone == phone && !m.name.is_empty())
                            .map(|m| m.name.clone())
                            .unwrap_or_default(),
                        text: text.clone(),
                        timestamp: Local::now().timestamp(),
                        outgoing: true,
                        read: true,
                        state: SendState::Pending,
                    };
                    cache.push(message);
                    save(&cache, &mut saved_cache, settings.save_history);
                    emit(Event::Snapshot(cache.clone()));
                    // No retries: a lost acknowledgement must never duplicate a real message.
                    let session = transport::find_iphone().and_then(MapClient::connect);
                    let message = cache.iter_mut().find(|m| m.id == id).unwrap();
                    let completion = match submit_once(session, &target, &text) {
                        Submission::Accepted(handle) => {
                            if !handle.is_empty() {
                                message.id = format!("sent:{handle}");
                            }
                            message.handle = handle;
                            message.state = SendState::Accepted;
                            Event::Sent(phone)
                        }
                        Submission::Unconfirmed => {
                            message.state = SendState::Unconfirmed;
                            Event::SendFailed("Проверь сообщение на iPhone перед повтором: оно могло уйти. Текст сохранён.".into())
                        }
                        Submission::Failed(error) => {
                            message.state = SendState::Failed;
                            was_connected = false;
                            emit(Event::Connection(false, error.to_string()));
                            emit(Event::ReconnectIn(
                                schedule.completed(false, settings.poll_seconds),
                            ));
                            Event::SendFailed("Не отправлено: нет соединения с iPhone. Текст сохранён; подключись и повтори вручную.".into())
                        }
                    };
                    save(&cache, &mut saved_cache, settings.save_history);
                    emit(Event::Snapshot(cache.clone()));
                    // The UI may exit on this event: persist the outcome first.
                    emit(completion);
                }
            }
        }
    });
    (commands, events)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reconnect_backoff_is_bounded_and_success_restores_normal_polling() {
        let mut schedule = PollSchedule::new();
        assert!(schedule.remaining().is_zero());
        for seconds in [3, 6, 12, 24, 30, 30, 30] {
            assert_eq!(schedule.completed(false, 3), seconds);
            assert!(schedule.remaining() <= Duration::from_secs(seconds));
        }
        assert_eq!(schedule.completed(true, 2), 2);
        assert_eq!(schedule.completed(false, 2), 3);
        schedule.next = SystemTime::now() - Duration::from_secs(3600);
        assert!(
            schedule.remaining().is_zero(),
            "Resume after sleep must refresh immediately"
        );
    }
    #[test]
    fn submission_distinguishes_preflight_failure_from_a_lost_ack_without_retry() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        struct Fake {
            calls: Arc<AtomicUsize>,
            lost: bool,
        }
        impl transport::Exchange for Fake {
            fn exchange(&mut self, packet: &[u8]) -> io::Result<Vec<u8>> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                if packet[0] == 0x80 {
                    return Ok(vec![0xa0, 0, 7, 0x10, 0, 0x10, 0]);
                }
                if packet[0] == 0x82 && self.lost {
                    return Err(io::Error::new(io::ErrorKind::TimedOut, "lost ack"));
                }
                Ok(vec![0xa0, 0, 3])
            }
        }
        let missing: io::Result<MapClient<Fake>> = Err(io::Error::other("offline"));
        assert!(matches!(
            submit_once(missing, "+79991234567", "test"),
            Submission::Failed(_)
        ));
        for lost in [false, true] {
            let calls = Arc::new(AtomicUsize::new(0));
            let session = MapClient::open(Fake {
                calls: calls.clone(),
                lost,
            });
            assert_eq!(calls.load(Ordering::SeqCst), 4);
            let result = submit_once(session, "+79991234567", "test");
            assert_eq!(
                calls.load(Ordering::SeqCst),
                5,
                "Exactly one upload, even if its acknowledgement is lost"
            );
            assert!(if lost {
                matches!(result, Submission::Unconfirmed)
            } else {
                matches!(result, Submission::Accepted(_))
            });
        }
    }
    #[test]
    fn parses_real_map_attributes_without_losing_unicode() {
        let xml = r#"<?xml version="1.0"?><MAP-msg-listing version="1.0"><msg handle="ABC001" sender_addressing="+79991234567" sender_name="Анна" datetime="20261102T145600" subject="Привет &amp; пока" read="yes"/></MAP-msg-listing>"#;
        let messages = listing(xml, false).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].text, "Привет & пока");
        assert_eq!(messages[0].name, "Анна");
        assert!(messages[0].read);
    }
}
