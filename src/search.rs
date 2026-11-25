use crate::{
    model::{self, Conversation, Message},
    settings::Settings,
};

#[derive(Clone, Debug)]
pub struct Hit {
    pub message_id: String,
    pub phone: String,
    pub name: String,
    pub text: String,
    pub timestamp: i64,
}

pub fn matches_contact(query: &str, conversation: &Conversation) -> bool {
    let query = query.trim().to_lowercase();
    let digits: String = query.chars().filter(char::is_ascii_digit).collect();
    conversation.name.to_lowercase().contains(&query)
        || conversation.phone.to_lowercase().contains(&query)
        || (!digits.is_empty()
            && query
                .chars()
                .all(|c| c.is_ascii_digit() || "+()- ".contains(c))
            && model::key(&conversation.phone).contains(&digits))
}

pub fn messages(
    query: &str,
    records: &[Message],
    chats: &[Conversation],
    unread_only: bool,
) -> Vec<Hit> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Vec::new();
    }
    let chats: std::collections::HashMap<_, _> = chats
        .iter()
        .map(|chat| (chat.phone.as_str(), chat))
        .collect();
    let mut hits: Vec<_> = records
        .iter()
        .filter_map(|message| {
            if !message.text.to_lowercase().contains(&query) {
                return None;
            }
            let chat = chats.get(message.phone.as_str())?;
            if unread_only && !chat.unread {
                return None;
            }
            Some(Hit {
                message_id: message.id.clone(),
                phone: message.phone.clone(),
                name: chat.name.clone(),
                text: message.text.replace(['\r', '\n'], " "),
                timestamp: message.timestamp,
            })
        })
        .collect();
    hits.sort_by(|a, b| {
        b.timestamp
            .cmp(&a.timestamp)
            .then_with(|| a.message_id.cmp(&b.message_id))
    });
    hits
}

pub fn sort_chats(chats: &mut [Conversation], settings: &Settings) {
    chats.sort_by_key(|chat| {
        (
            !settings
                .chats
                .get(&chat.phone)
                .is_some_and(|entry| entry.pinned),
            std::cmp::Reverse(chat.time),
            chat.phone.clone(),
        )
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SendState;
    #[test]
    fn searches_old_message_and_formatted_phone_and_keeps_stable_order() {
        let records: Vec<_> = ["Привет ❤️", "Другое", "ПРИВЕТ ещё"]
            .into_iter()
            .enumerate()
            .map(|(i, text)| Message {
                id: i.to_string(),
                handle: String::new(),
                phone: "+79991234567".into(),
                name: "Анна".into(),
                text: text.into(),
                timestamp: i as i64,
                outgoing: false,
                read: true,
                state: SendState::Received,
            })
            .collect();
        let chats = model::conversations(&records);
        let hits = messages(" привет ", &records, &chats, false);
        assert_eq!(
            hits.iter()
                .map(|hit| hit.message_id.as_str())
                .collect::<Vec<_>>(),
            ["2", "0"]
        );
        assert!(matches_contact("+7 (999) 123", &chats[0]));
        assert!(messages("привет", &records, &chats, true).is_empty());
        assert!(messages("  ", &records, &chats, false).is_empty());
    }
}
