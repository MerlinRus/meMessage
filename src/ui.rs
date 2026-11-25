#![allow(deprecated)] // Preserve stable widget identities across the egui 0.37 migration.
use crate::{
    contacts::{self, Book},
    emoji::{self, Catalog},
    layout, map,
    model::{self, Conversation, Message, SendState},
    worker::{self, Command, Event},
};
use chrono::{Local, TimeZone};
use eframe::egui::{
    epaint::text::{LayoutJob, TextWrapping},
    *,
};
use std::{
    collections::HashMap,
    fs,
    sync::{
        Arc, Mutex,
        mpsc::{Receiver, Sender},
    },
    time::Duration,
};

const BLUE: Color32 = Color32::from_rgb(7, 131, 255);
const TEXT: Color32 = Color32::from_rgb(29, 29, 31);
const SECONDARY: Color32 = Color32::from_rgb(116, 116, 122);
const SIDE: Color32 = Color32::from_rgb(246, 246, 248);

use crate::icons::{Icon, paint as paint_icon};

struct PhotoResult {
    phone: String,
    result: Result<Option<Vec<u8>>, String>,
}

pub struct MessagesApp {
    messages: Vec<Message>,
    conversations: Vec<Conversation>,
    selected: Option<String>,
    search: String,
    draft: String,
    recipient: String,
    drafts: HashMap<String, String>,
    new_mode: bool,
    unread_only: bool,
    preview: bool,
    sending: bool,
    connected: bool,
    connecting: bool,
    status: String,
    error: Option<String>,
    commands: Option<Sender<Command>>,
    events: Option<Receiver<Event>>,
    scroll_to_bottom: bool,
    focus_search: bool,
    focus_recipient: bool,
    emoji_catalog: Option<Arc<Catalog>>,
    emoji_query: String,
    emoji_category: String,
    notification_error: Option<String>,
    connection_error: Option<String>,
    contacts: Book,
    shared_contacts: Arc<Mutex<Book>>,
    contact_name: String,
    contacts_status: String,
    contacts_syncing: bool,
    motion: crate::motion::Motion,
    focus_draft: bool,
    submitted: Option<(String, String)>,
    preferences: crate::settings::Settings,
    shared_preferences: Arc<Mutex<crate::settings::Settings>>,
    settings_open: bool,
    settings_draft: crate::settings::Settings,
    settings_status: String,
    photo_events: Option<Receiver<PhotoResult>>,
    photo_loading: bool,
    photo_textures: HashMap<String, TextureHandle>,
    shared_active: Arc<Mutex<Option<String>>>,
    jump_to: Option<String>,
    highlighted: Option<String>,
    retry: Option<Message>,
    reconnect_seconds: Option<u64>,
    desktop: Option<crate::desktop::Desktop>,
    hidden: bool,
    exit_requested: bool,
    unread_count: Option<usize>,
    search_cache: Option<(String, bool, Vec<crate::search::Hit>)>,
    search_limit: usize,
}

pub fn setup(ctx: &Context) {
    ctx.set_theme(Theme::Light);
    egui_extras::install_image_loaders(ctx);
    let mut fonts = FontDefinitions::default();
    let font_root = model::app_root().join("assets/fonts");
    for (name, path) in [
        ("SF Pro", font_root.join("SF-Pro-Text-Regular.otf")),
        ("Semibold", font_root.join("SF-Pro-Text-Semibold.otf")),
        (
            "Segoe",
            std::path::PathBuf::from(r"C:\Windows\Fonts\segoeui.ttf"),
        ),
    ] {
        if let Ok(bytes) = fs::read(path) {
            fonts
                .font_data
                .insert(name.into(), Arc::new(FontData::from_owned(bytes)));
        }
    }
    for name in ["Segoe", "SF Pro"] {
        if fonts.font_data.contains_key(name) {
            fonts
                .families
                .get_mut(&FontFamily::Proportional)
                .unwrap()
                .insert(0, name.into());
        }
    }
    let mut semibold = Vec::new();
    if fonts.font_data.contains_key("Semibold") {
        semibold.push("Semibold".into());
    }
    semibold.extend(fonts.families[&FontFamily::Proportional].clone());
    fonts
        .families
        .insert(FontFamily::Name("Semibold".into()), semibold);
    fonts.families.insert(
        FontFamily::Monospace,
        fonts.families[&FontFamily::Proportional].clone(),
    );
    ctx.set_fonts(fonts);
    let mut style = (*ctx.style_of(Theme::Light)).clone();
    style.visuals = Visuals::light();
    style.visuals.override_text_color = Some(TEXT);
    style.visuals.weak_text_color = Some(SECONDARY);
    style.visuals.selection.bg_fill = BLUE;
    style.visuals.selection.stroke = Stroke::new(1.0_f32, Color32::WHITE);
    style.visuals.panel_fill = Color32::WHITE;
    style.visuals.text_cursor.stroke = Stroke::new(1.5_f32, BLUE);
    style.spacing.item_spacing = vec2(8., 4.);
    style
        .text_styles
        .insert(TextStyle::Body, FontId::proportional(layout::BODY_SIZE));
    style
        .text_styles
        .insert(TextStyle::Button, FontId::proportional(13.));
    style.text_styles.insert(
        TextStyle::Heading,
        FontId::new(22., FontFamily::Name("Semibold".into())),
    );
    style
        .text_styles
        .insert(TextStyle::Small, FontId::proportional(11.));
    style.text_styles.insert(
        TextStyle::Monospace,
        FontId::proportional(layout::BODY_SIZE),
    );
    ctx.set_style_of(Theme::Light, style);
}

impl MessagesApp {
    pub fn new(cc: &eframe::CreationContext<'_>, preview: bool) -> Self {
        let rounded = crate::platform::round_window(cc);
        let mut app = Self::from_context(&cc.egui_ctx, preview);
        if let Err(error) = rounded {
            app.settings_status = format!("Скругление окна недоступно: {error}");
        }
        if !preview || std::env::args().any(|arg| arg == "--tray-preview") {
            match crate::desktop::Desktop::start(cc) {
                Ok(desktop) => app.desktop = Some(desktop),
                Err(error) => app.settings_status = format!("Значок в трее недоступен: {error}"),
            }
            if !preview {
                app.preferences.autostart = crate::desktop::autostart_enabled();
                app.settings_draft = app.preferences.clone();
                if let Ok(mut shared) = app.shared_preferences.lock() {
                    *shared = app.preferences.clone();
                }
            }
            if app.desktop.as_ref().is_some_and(|tray| tray.available())
                && std::env::args().any(|arg| arg == "--background")
            {
                app.hidden = true;
                cc.egui_ctx
                    .send_viewport_cmd(ViewportCommand::Visible(false));
            }
        }
        app
    }
    fn from_context(ctx: &Context, preview: bool) -> Self {
        setup(ctx);
        let migration = if preview {
            Ok(0)
        } else {
            model::migrate_legacy()
        };
        let loaded = if preview {
            Ok(crate::settings::Settings::default())
        } else {
            crate::settings::Settings::load(&model::data_dir().join("settings.bin"))
        };
        let settings_status = if let Err(error) = &loaded {
            format!("Настройки не открылись: {error}")
        } else if let Err(error) = migration {
            format!("Не удалось перенести данные: {error}")
        } else {
            String::new()
        };
        let preferences = loaded.unwrap_or_default();
        let shared_preferences = Arc::new(Mutex::new(preferences.clone()));
        let mut motion = crate::motion::Motion::new();
        motion.enabled &= preferences.animations;
        ctx.style_mut_of(Theme::Light, |style| {
            style.animation_time = if motion.enabled {
                crate::motion::POPUP_TIME
            } else {
                0.
            }
        });
        let emoji_root = model::app_root().join("assets/emoji");
        let emoji_catalog = Catalog::load(&emoji_root).ok();
        if let Some(catalog) = &emoji_catalog {
            catalog.install(ctx);
        }
        let (contacts, contacts_status) = if preview {
            (Book::default(), String::new())
        } else {
            match Book::load(&contacts::path()) {
                Ok(book) => (book, String::new()),
                Err(_) => (
                    Book::default(),
                    "Не удалось открыть сохранённые контакты.".into(),
                ),
            }
        };
        let shared_contacts = Arc::new(Mutex::new(contacts.clone()));
        let shared_active = Arc::new(Mutex::new(None));
        let (commands, events) = if preview {
            (None, None)
        } else {
            let (c, e) = worker::start(
                ctx.clone(),
                model::data_path(),
                shared_contacts.clone(),
                shared_preferences.clone(),
                shared_active.clone(),
            );
            (Some(c), Some(e))
        };
        let mut app = Self {
            messages: vec![],
            conversations: vec![],
            selected: None,
            search: String::new(),
            draft: String::new(),
            recipient: String::new(),
            drafts: HashMap::new(),
            new_mode: false,
            unread_only: false,
            preview,
            sending: false,
            connected: false,
            connecting: !preview,
            status: if preview {
                "Предпросмотр · без подключения"
            } else {
                "Подключение к iPhone…"
            }
            .into(),
            error: None,
            commands,
            events,
            scroll_to_bottom: true,
            focus_search: false,
            focus_recipient: false,
            emoji_catalog,
            emoji_query: String::new(),
            emoji_category: String::new(),
            notification_error: None,
            connection_error: None,
            contacts,
            shared_contacts,
            contact_name: String::new(),
            contacts_status,
            contacts_syncing: false,
            motion,
            focus_draft: false,
            submitted: None,
            settings_draft: preferences.clone(),
            preferences,
            shared_preferences,
            settings_open: false,
            settings_status,
            photo_events: None,
            photo_loading: false,
            photo_textures: HashMap::new(),
            shared_active,
            jump_to: None,
            highlighted: None,
            retry: None,
            reconnect_seconds: None,
            desktop: None,
            hidden: false,
            exit_requested: false,
            unread_count: None,
            search_cache: None,
            search_limit: 100,
        };
        if preview {
            app.seed_preview();
        } else if app.preferences.save_drafts {
            match crate::settings::Drafts::load(&model::data_dir().join("drafts.bin")) {
                Ok(saved) => {
                    app.drafts = saved.messages;
                    app.selected = saved.selected;
                    app.new_mode = saved.new_mode;
                    app.recipient = saved.recipient;
                    if let Some((phone, text)) = saved.submitted {
                        app.drafts.entry(phone).or_insert(text);
                    }
                    if let Some(phone) = &app.selected {
                        app.draft = app.drafts.remove(phone).unwrap_or_default();
                    } else if app.new_mode {
                        app.draft = app.drafts.remove("").unwrap_or_default();
                    }
                    if saved.uncertain_submission {
                        app.error = Some(
                            "Предыдущая отправка могла завершиться. Проверь iPhone перед повтором."
                                .into(),
                        );
                    }
                }
                Err(error) => app.error = Some(format!("Черновики не открылись: {error}")),
            }
        }
        app
    }
    fn receive(&mut self, ctx: &Context) {
        let actions: Vec<_> = self
            .desktop
            .as_ref()
            .map(|tray| tray.events.try_iter().collect())
            .unwrap_or_default();
        for action in actions {
            match action {
                crate::desktop::Action::Open => {
                    self.hidden = false;
                    self.focus_draft = !self.settings_open;
                }
                crate::desktop::Action::Settings => {
                    self.hidden = false;
                    self.open_settings();
                }
                crate::desktop::Action::Exit => {
                    self.hidden = false;
                    self.exit_requested = true;
                }
                crate::desktop::Action::Resume => {
                    if let Some(commands) = &self.commands {
                        let _ = commands.send(Command::Refresh);
                    }
                }
            }
        }
        if self.hidden && !self.desktop.as_ref().is_some_and(|tray| tray.available()) {
            self.hidden = false;
            ctx.send_viewport_cmd(ViewportCommand::Visible(true));
            self.error = Some("Значок в трее недоступен. Окно снова открыто.".into());
        }
        if let Some(result) = self.photo_events.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.photo_events = None;
            self.photo_loading = false;
            match result.result {
                Ok(Some(bytes)) => {
                    self.set_photo(&result.phone, Some(bytes));
                }
                Ok(None) => {}
                Err(error) => {
                    self.contacts_status = error.clone();
                    self.error = Some(error);
                }
            }
        }
        let events: Vec<Event> = self
            .events
            .as_ref()
            .map(|r| r.try_iter().collect())
            .unwrap_or_default();
        for event in events {
            match event {
                Event::Connecting => {
                    self.connecting = true;
                    if !self.connected {
                        self.status = "Подключение к iPhone…".into();
                    }
                }
                Event::StorageError(error) => self.error = Some(error),
                Event::Snapshot(mut messages) => {
                    model::preserve_read(&self.messages, &mut messages);
                    if self.connected {
                        self.motion.register(&self.messages, &messages);
                    }
                    self.messages = messages;
                    self.rebuild_conversations();
                    if self.selected.is_none()
                        && !self.new_mode
                        && let Some(c) = self.conversations.first()
                    {
                        let phone = c.phone.clone();
                        self.selected = Some(phone.clone());
                        self.draft = self.drafts.remove(&phone).unwrap_or_default();
                        self.focus_draft = !self.settings_open;
                        self.scroll_to_bottom = true;
                    }
                }
                Event::Connection(connected, text) => {
                    self.reconnect_seconds = None;
                    self.connecting = false;
                    self.connected = connected;
                    if connected {
                        self.status = text;
                        self.connection_error = None;
                    } else {
                        self.status = "Нет связи с iPhone".into();
                        self.connection_error = Some(text);
                    }
                }
                Event::ReconnectIn(seconds) => self.reconnect_seconds = Some(seconds),
                Event::Sent(phone) => {
                    self.sending = false;
                    if self
                        .submitted
                        .as_ref()
                        .is_some_and(|(target, _)| target == &phone)
                    {
                        self.submitted = None;
                    }
                    self.scroll_to_bottom = true;
                }
                Event::OpenConversation(phone) => {
                    self.hidden = false;
                    self.settings_open = false;
                    self.select(&phone);
                    ctx.send_viewport_cmd(ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(ViewportCommand::Focus);
                }
                Event::NotificationError(error) => {
                    self.notification_error = Some(error);
                }
                Event::Contacts(book) => {
                    self.photo_textures.clear();
                    self.contacts = self
                        .shared_contacts
                        .lock()
                        .map(|current| current.clone())
                        .unwrap_or(book);
                    self.rebuild_conversations();
                }
                Event::ContactsStatus(busy, status) => {
                    self.contacts_syncing = busy;
                    self.contacts_status = status;
                }
                Event::SendFailed(error) => {
                    self.sending = false;
                    if let Some((phone, text)) = self.submitted.take() {
                        if self.selected.as_deref() == Some(&phone) {
                            if self.draft.is_empty() {
                                self.draft = text;
                                self.focus_draft = true;
                            }
                        } else if self.drafts.get(&phone).is_none_or(String::is_empty) {
                            self.drafts.insert(phone, text);
                        }
                    }
                    self.error = Some(error);
                }
            }
        }
        self.update_shell(ctx);
        self.handle_lifecycle(ctx);
    }
    fn handle_lifecycle(&mut self, ctx: &Context) {
        if self.exit_requested && !self.sending {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
        if !self.exit_requested
            && self.preferences.minimize_on_close
            && self.desktop.as_ref().is_some_and(|tray| tray.available())
            && ctx.input(|i| i.viewport().close_requested())
        {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.hidden = true;
            if let Ok(mut active) = self.shared_active.lock() {
                *active = None;
            }
            ctx.send_viewport_cmd(ViewportCommand::Visible(false));
        } else if self.sending && ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.error = Some("Дождись результата отправки, затем закрой окно.".into());
        }
    }
    fn update_shell(&mut self, ctx: &Context) {
        let unread = self
            .messages
            .iter()
            .filter(|m| !m.outgoing && !m.read)
            .count();
        if self.unread_count != Some(unread) {
            self.unread_count = Some(unread);
            if !self.preview {
                ctx.send_viewport_cmd(ViewportCommand::Title(if unread == 0 {
                    "meMessage".into()
                } else {
                    format!("({unread}) meMessage")
                }));
            }
        }
        if let Some(desktop) = &mut self.desktop {
            desktop.update(unread, self.connected);
        }
    }
    fn rebuild_conversations(&mut self) {
        self.search_cache = None;
        self.conversations = model::conversations(&self.messages);
        self.contacts.apply(&mut self.conversations);
        crate::search::sort_chats(&mut self.conversations, &self.preferences);
    }
    fn connect(&mut self) {
        if self.preview || self.connected || self.connecting {
            return;
        }
        if self
            .commands
            .as_ref()
            .is_some_and(|commands| commands.send(Command::Connect).is_ok())
        {
            self.connecting = true;
            self.status = "Подключение к iPhone…".into();
            self.connection_error = None;
        } else {
            self.connection_error =
                Some("Служба подключения остановлена. Перезапусти приложение.".into());
        }
    }
    fn mark_selected_read(&mut self) {
        let Some(phone) = &self.selected else {
            return;
        };
        let ids: Vec<_> = self
            .messages
            .iter()
            .filter(|message| message.phone == *phone && !message.outgoing && !message.read)
            .map(|message| message.id.clone())
            .collect();
        if ids.is_empty() {
            return;
        }
        if !self.preview
            && !self
                .commands
                .as_ref()
                .is_some_and(|commands| commands.send(Command::MarkRead(ids.clone())).is_ok())
        {
            return;
        }
        if model::mark_read(&mut self.messages, &ids) {
            self.rebuild_conversations();
        }
    }
    fn contact_phone(&self) -> Option<String> {
        self.selected.clone().or_else(|| {
            map::normalize_recipient(&self.recipient)
                .ok()
                .map(|phone| model::key(&phone))
        })
    }
    fn avatar(&mut self, ui: &Ui, phone: &str, center: Pos2, radius: f32) {
        let phone = model::key(phone);
        if !self.photo_textures.contains_key(&phone)
            && let Some(bytes) = self.contacts.photos.get(&phone)
            && let Some(texture) = crate::avatars::texture(ui.ctx(), bytes)
        {
            self.photo_textures.insert(phone.clone(), texture);
        }
        if let Some(texture) = self.photo_textures.get(&phone) {
            crate::avatars::paint(ui.painter(), texture, center, radius);
        } else {
            paint_icon(
                ui,
                Rect::from_center_size(center, Vec2::splat(radius * 2.)),
                Icon::Person,
                Color32::WHITE,
            );
        }
    }
    fn set_photo(&mut self, phone: &str, photo: Option<Vec<u8>>) -> bool {
        let result = (|| -> std::io::Result<Book> {
            let mut shared = self
                .shared_contacts
                .lock()
                .map_err(|_| std::io::Error::other("Контакты заняты."))?;
            let mut updated = shared.clone();
            let phone = model::key(phone);
            if let Some(bytes) = photo {
                updated.photos.insert(phone, bytes);
            } else {
                updated.photos.remove(&phone);
            }
            if !self.preview {
                updated.save(&contacts::path())?;
            }
            *shared = updated.clone();
            Ok(updated)
        })();
        match result {
            Ok(book) => {
                self.contacts = book;
                self.photo_textures.remove(&model::key(phone));
                self.contacts_status = "Фото контакта сохранено на ноутбуке.".into();
                true
            }
            Err(error) => {
                self.contacts_status = format!("Фото не сохранено: {error}");
                false
            }
        }
    }
    fn choose_photo(&mut self, phone: &str, ctx: &Context) {
        if self.photo_loading {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        self.photo_events = Some(rx);
        self.photo_loading = true;
        self.contacts_status = "Выбери фото. Оно сохранится только на ноутбуке.".into();
        let phone = model::key(phone);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = rfd::FileDialog::new()
                .set_title("Фото контакта")
                .add_filter("Фотографии", &["jpg", "jpeg", "png", "webp"])
                .pick_file()
                .map(|path| crate::avatars::from_file(&path).map_err(|error| error.to_string()))
                .transpose();
            let _ = tx.send(PhotoResult { phone, result });
            ctx.request_repaint();
        });
    }
    fn prepare_contact(&mut self) {
        self.contact_name = self
            .contact_phone()
            .map(|phone| {
                let name = self.contacts.display(&phone, &self.name());
                if contacts::meaningful(&name, &phone) {
                    name
                } else {
                    String::new()
                }
            })
            .unwrap_or_default();
    }
    fn save_contact(&mut self, phone: &str, name: &str) -> bool {
        let updated = (|| -> std::io::Result<Book> {
            let mut shared = self
                .shared_contacts
                .lock()
                .map_err(|_| std::io::Error::other("Контакты заняты."))?;
            let mut updated = shared.clone();
            updated.set_alias(phone, name)?;
            if !self.preview {
                updated.save(&contacts::path())?;
            }
            *shared = updated.clone();
            Ok(updated)
        })();
        match updated {
            Ok(book) => {
                self.contacts = book;
                self.rebuild_conversations();
                self.contacts_status = "Имя сохранено на ноутбуке.".into();
                true
            }
            Err(error) => {
                self.contacts_status = error.to_string();
                false
            }
        }
    }
    fn contact_menu(&mut self, ui: &mut Ui) {
        ui.set_width(280.);
        if let Some(phone) = self.contact_phone() {
            ui.label(RichText::new(self.contacts.display(&phone, &self.name())).strong());
            ui.label(
                RichText::new(format!("Получатель: {}", model::format_phone(&phone)))
                    .small()
                    .color(SECONDARY),
            );
            let related = self.contacts.related_addresses(&phone);
            if !related.is_empty() {
                ui.label(
                    RichText::new("Другие адреса контакта")
                        .small()
                        .color(SECONDARY),
                );
                for address in related.iter().filter(|address| *address != &phone) {
                    ui.label(
                        RichText::new(model::format_phone(address))
                            .small()
                            .color(SECONDARY),
                    );
                }
            }
            ui.add_space(8.);
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(44., 44.), Sense::hover());
                self.avatar(ui, &phone, rect.center(), 22.);
                if ui
                    .add_enabled(
                        !self.photo_loading,
                        Button::image_and_text(
                            Image::from_bytes(Icon::Photo.source().0, Icon::Photo.source().1)
                                .fit_to_exact_size(vec2(14., 14.))
                                .tint(TEXT),
                            if self.contacts.photos.contains_key(&model::key(&phone)) {
                                "Изменить фото"
                            } else {
                                "Добавить фото"
                            },
                        ),
                    )
                    .clicked()
                {
                    self.choose_photo(&phone, ui.ctx());
                }
                if self.contacts.photos.contains_key(&model::key(&phone))
                    && ui.button("Убрать фото").clicked()
                {
                    self.set_photo(&phone, None);
                }
            });
            ui.add_space(10.);
            ui.label("Имя контакта");
            ui.add(
                TextEdit::singleline(&mut self.contact_name)
                    .hint_text("Как подписать контакт")
                    .char_limit(100)
                    .desired_width(f32::INFINITY),
            );
            if ui.button("Сохранить имя").clicked() {
                let name = self.contact_name.clone();
                if self.save_contact(&phone, &name) {
                    ui.close();
                }
            }
            if self.contacts.aliases.contains_key(&model::key(&phone))
                && ui.button("Использовать имя iPhone").clicked()
                && self.save_contact(&phone, "")
            {
                self.prepare_contact();
                ui.close();
            }
            ui.label(
                RichText::new("Это имя сохранится на ноутбуке. Контакт на iPhone не изменится.")
                    .small()
                    .color(SECONDARY),
            );
            self.chat_actions(ui, &phone);
            ui.separator();
        }
        if ui
            .add_enabled(
                !self.preview && !self.contacts_syncing,
                Button::new(if self.contacts_syncing {
                    "Синхронизация…"
                } else {
                    "Обновить контакты с iPhone"
                }),
            )
            .clicked()
        {
            if let Some(commands) = &self.commands {
                let _ = commands.send(Command::SyncContacts);
            }
            self.contacts_syncing = true;
        }
        if !self.contacts_status.is_empty() {
            ui.label(
                RichText::new(&self.contacts_status)
                    .small()
                    .color(SECONDARY),
            );
        }
    }
    fn select(&mut self, phone: &str) {
        self.jump_to = None;
        self.highlighted = None;
        self.focus_draft = !self.settings_open;
        if self.selected.as_deref() != Some(phone) {
            self.motion.switch_dialogue();
        }
        self.drafts.insert(
            self.selected.clone().unwrap_or_default(),
            self.draft.clone(),
        );
        self.selected = Some(phone.into());
        self.draft = self.drafts.get(phone).cloned().unwrap_or_default();
        self.new_mode = false;
        self.error = None;
        self.scroll_to_bottom = true;
    }
    fn chat_icon_width(&self, phone: &str) -> f32 {
        self.preferences.chats.get(phone).map_or(0., |chat| {
            f32::from(u8::from(chat.pinned) + u8::from(chat.muted)) * 16.
        })
    }
    fn chat_icons(&self, ui: &Ui, row: Rect, phone: &str, color: Color32) {
        if let Some(chat) = self.preferences.chats.get(phone) {
            let mut right = row.right() - 13.;
            for (visible, icon) in [(chat.muted, Icon::BellOff), (chat.pinned, Icon::Pin)] {
                if visible {
                    paint_icon(
                        ui,
                        Rect::from_center_size(pos2(right, row.top() + 44.), vec2(12., 12.)),
                        icon,
                        color,
                    );
                    right -= 16.;
                }
            }
        }
    }
    fn chat_actions(&mut self, ui: &mut Ui, phone: &str) {
        let current = self
            .preferences
            .chats
            .get(phone)
            .cloned()
            .unwrap_or_default();
        if ui
            .button(if current.pinned {
                "Открепить диалог"
            } else {
                "Закрепить диалог"
            })
            .clicked()
        {
            self.change_chat(phone, !current.pinned, current.muted);
            ui.close();
        }
        if ui
            .button(if current.muted {
                "Включить уведомления"
            } else {
                "Без уведомлений"
            })
            .clicked()
        {
            self.change_chat(phone, current.pinned, !current.muted);
            ui.close();
        }
        let copy_label = if phone.contains('@') {
            "Копировать email"
        } else {
            "Копировать номер"
        };
        if ui.button(copy_label).clicked() {
            ui.ctx().copy_text(model::format_phone(phone));
            ui.close();
        }
    }
    fn change_chat(&mut self, phone: &str, pinned: bool, muted: bool) {
        let mut preferences = self.preferences.clone();
        let phone = model::key(phone);
        if pinned || muted {
            preferences
                .chats
                .insert(phone, crate::settings::ChatSettings { pinned, muted });
        } else {
            preferences.chats.remove(&phone);
        }
        if !self.preview
            && let Err(error) = preferences.save(&model::data_dir().join("settings.bin"))
        {
            self.error = Some(format!("Не удалось сохранить настройки диалога: {error}"));
            return;
        }
        self.preferences = preferences;
        self.settings_draft.chats = self.preferences.chats.clone();
        if let Ok(mut shared) = self.shared_preferences.lock() {
            *shared = self.preferences.clone();
        }
        self.rebuild_conversations();
    }
    fn retry_dialog(&mut self, ctx: &Context) {
        let Some(message) = self.retry.clone() else {
            return;
        };
        Modal::new(Id::new("retry_send")).show(ctx,|ui| {
            ui.set_width(340.);
            ui.heading("Повторить отправку?");
            ui.label(if message.state==SendState::Unconfirmed {
                "iPhone мог уже отправить это сообщение. Сначала проверь переписку на телефоне: повтор может создать дубликат."
            } else {"Сообщение не было передано iPhone. Повтор отправит его ещё раз."});
            ui.add_space(8.);
            ui.label(RichText::new(self.contacts.display(&message.phone,&message.name)).strong());
            ScrollArea::vertical().max_height(130.).show(ui,|ui|{ui.label(&message.text);});
            ui.add_space(8.);
            ui.horizontal(|ui| {
                if ui.add_enabled(!self.sending && !self.preview,Button::new("Отправить ещё раз")).clicked() {
                    match map::normalize_recipient(&message.phone) {
                        Ok(target) => {
                            if self.commands.as_ref().is_some_and(|commands| commands.send(Command::Send{
                                phone:message.phone.clone(),target,text:message.text.clone(),
                            }).is_ok()) {
                                self.submitted=Some((message.phone.clone(),message.text.clone()));
                                self.sending=true;
                                self.error=None;
                                self.retry=None;
                            } else { self.error=Some("Служба отправки остановлена. Перезапусти приложение.".into());self.retry=None; }
                        }
                        Err(error)=>{self.error=Some(error.to_string());self.retry=None;}
                    }
                }
                if ui.button("Отмена").clicked() { self.retry=None; }
            });
        });
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            self.retry = None;
        }
    }
    fn new_conversation(&mut self) {
        self.jump_to = None;
        self.highlighted = None;
        self.focus_draft = false;
        self.drafts.insert(
            self.selected.clone().unwrap_or_default(),
            self.draft.clone(),
        );
        self.selected = None;
        self.new_mode = true;
        self.recipient.clear();
        self.draft = self.drafts.get("").cloned().unwrap_or_default();
        self.focus_recipient = true;
        self.error = None;
    }
    fn send(&mut self) {
        if self.sending || self.preview || self.draft.trim().is_empty() {
            return;
        }
        let input = self.selected.as_deref().unwrap_or(&self.recipient);
        let target = match map::normalize_recipient(input) {
            Ok(target) => target,
            Err(error) => {
                self.error = Some(error.to_string());
                return;
            }
        };
        let phone = model::key(&target);
        if let Err(error) = map::bmessage(&target, &self.draft) {
            self.error = Some(error.to_string());
            return;
        }
        if let Some(commands) = &self.commands {
            if commands
                .send(Command::Send {
                    phone: phone.clone(),
                    target,
                    text: self.draft.clone(),
                })
                .is_ok()
            {
                self.submitted = Some((phone.clone(), std::mem::take(&mut self.draft)));
                self.drafts.remove(&phone);
                self.selected = Some(phone);
                self.new_mode = false;
                self.sending = true;
                self.focus_draft = true;
                self.error = None;
                self.scroll_to_bottom = true;
            } else {
                self.error = Some("Соединение остановлено. Перезапусти приложение.".into());
            }
        }
    }
    fn name(&self) -> String {
        self.selected
            .as_deref()
            .and_then(|p| self.conversations.iter().find(|c| c.phone == p))
            .map(|c| c.name.clone())
            .unwrap_or_else(|| {
                self.selected
                    .as_deref()
                    .map(model::format_phone)
                    .unwrap_or("Сообщения".into())
            })
    }
    fn sidebar(&mut self, ui: &mut Ui, rect: Rect) {
        ui.painter().rect_filled(rect, 0, SIDE);
        let drag = Rect::from_min_max(
            pos2(rect.left() + 76., rect.top()),
            pos2(rect.right() - 40., rect.top() + 35.),
        );
        drag_area(ui, drag, "sidebar_drag");
        for (i, color, label) in [
            (0, Color32::from_rgb(255, 95, 87), "Закрыть"),
            (1, Color32::from_rgb(254, 188, 46), "Свернуть"),
            (2, Color32::from_rgb(40, 200, 64), "Развернуть"),
        ] {
            let center = pos2(rect.left() + 17. + i as f32 * 20., rect.top() + 17.);
            let response = ui
                .interact(
                    Rect::from_center_size(center, vec2(16., 20.)),
                    Id::new(("traffic", i)),
                    Sense::click(),
                )
                .on_hover_cursor(CursorIcon::PointingHand);
            response.widget_info(|| WidgetInfo::labeled(Role::Button, true, label));
            ui.painter().circle_filled(center, 6., color);
            if response.hovered() {
                paint_icon(
                    ui,
                    Rect::from_center_size(center, vec2(7., 7.)),
                    match i {
                        0 => Icon::Close,
                        1 => Icon::Minimize,
                        _ => Icon::Maximize,
                    },
                    Color32::from_black_alpha(150),
                );
            }
            if response.clicked() {
                match i {
                    0 => ui.ctx().send_viewport_cmd(ViewportCommand::Close),
                    1 => ui.ctx().send_viewport_cmd(ViewportCommand::Minimized(true)),
                    _ => {
                        let max = ui.input(|i| i.viewport().maximized.unwrap_or(false));
                        ui.ctx().send_viewport_cmd(ViewportCommand::Maximized(!max));
                    }
                }
            }
        }
        let filter = icon_button(
            ui,
            Rect::from_center_size(pos2(rect.right() - 22., rect.top() + 17.), vec2(30., 30.)),
            Icon::Filter,
            "Фильтр сообщений",
            true,
        );
        Popup::menu(&filter).show(|ui| {
            if ui
                .selectable_label(!self.unread_only, "Все сообщения")
                .clicked()
            {
                self.unread_only = false;
                ui.close();
            }
            if ui
                .selectable_label(self.unread_only, "Непрочитанные")
                .clicked()
            {
                self.unread_only = true;
                ui.close();
            }
            ui.separator();
            if ui.button("Обновить").clicked() {
                if let Some(c) = &self.commands {
                    let _ = c.send(Command::Refresh);
                }
                ui.close();
            }
        });
        let search = Rect::from_min_size(
            pos2(rect.left() + 10., rect.top() + 40.),
            vec2(rect.width() - 20., 30.),
        );
        ui.painter()
            .rect_filled(search, 15, Color32::from_rgb(234, 234, 237));
        paint_icon(
            ui,
            Rect::from_center_size(pos2(search.left() + 16., search.center().y), vec2(13., 13.)),
            Icon::Search,
            SECONDARY,
        );
        let response = ui.put(
            Rect::from_min_max(
                pos2(search.left() + 31., search.top() + 5.),
                pos2(
                    search.right() - if self.search.is_empty() { 11. } else { 32. },
                    search.bottom() - 3.,
                ),
            ),
            TextEdit::singleline(&mut self.search)
                .id(Id::new("search"))
                .hint_text("Поиск")
                .font(FontId::proportional(12.5))
                .frame(Frame::NONE),
        );
        if self.focus_search {
            response.request_focus();
            self.focus_search = false;
        }
        if !self.search.is_empty()
            && icon_button(
                ui,
                Rect::from_center_size(
                    pos2(search.right() - 15., search.center().y),
                    vec2(24., 24.),
                ),
                Icon::Close,
                "Очистить поиск",
                true,
            )
            .clicked()
        {
            self.search.clear();
            self.highlighted = None;
            self.focus_search = true;
        }
        let rows = Rect::from_min_max(
            pos2(rect.left() + 5., search.bottom() + 10.),
            pos2(
                rect.right() - 5.,
                rect.bottom()
                    - if !self.preview && !self.connected {
                        70.
                    } else {
                        30.
                    },
            ),
        );
        let query = self.search.to_lowercase();
        let filtered: Vec<Conversation> = self
            .conversations
            .iter()
            .filter(|c| {
                (!self.unread_only || c.unread)
                    && (query.is_empty() || crate::search::matches_contact(&query, c))
            })
            .cloned()
            .collect();
        if self
            .search_cache
            .as_ref()
            .is_none_or(|(cached, unread, _)| cached != &query || *unread != self.unread_only)
        {
            let hits = crate::search::messages(
                &query,
                &self.messages,
                &self.conversations,
                self.unread_only,
            );
            self.search_cache = Some((query.clone(), self.unread_only, hits));
            self.search_limit = 100;
        }
        let all_hits = &self
            .search_cache
            .as_ref()
            .expect("Search cache initialized")
            .2;
        let hit_count = all_hits.len();
        let hits: Vec<_> = all_hits.iter().take(self.search_limit).cloned().collect();
        let empty = filtered.is_empty() && hits.is_empty();
        ui.scope_builder(
            UiBuilder::new()
                .max_rect(rows)
                .layout(Layout::top_down(Align::Min)),
            |ui| {
                ScrollArea::vertical()
                    .id_salt("conversation_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 2.;
                        if !query.trim().is_empty() && !filtered.is_empty() {
                            ui.label(RichText::new("Контакты").small().color(SECONDARY));
                        }
                        for conversation in filtered {
                            let (row, response) = ui.allocate_exact_size(
                                vec2(rows.width() - 3., layout::ROW_HEIGHT),
                                Sense::click(),
                            );
                            response.widget_info(|| {
                                WidgetInfo::labeled(Role::ListBoxOption, true, &conversation.name)
                            });
                            let selected = self.selected.as_deref() == Some(&conversation.phone)
                                && !self.new_mode;
                            if selected || response.hovered() {
                                ui.painter().rect_filled(
                                    row,
                                    10,
                                    if selected {
                                        BLUE
                                    } else {
                                        Color32::from_rgb(233, 233, 237)
                                    },
                                );
                            }
                            self.avatar(
                                ui,
                                &conversation.phone,
                                pos2(row.left() + 30., row.center().y),
                                21.,
                            );
                            let foreground = if selected { Color32::WHITE } else { TEXT };
                            let secondary = if selected {
                                Color32::from_rgb(235, 245, 255)
                            } else {
                                SECONDARY
                            };
                            let time = model::time_label(conversation.time);
                            let time_galley = ui.painter().layout_no_wrap(
                                time,
                                FontId::proportional(10.5),
                                secondary,
                            );
                            ui.painter().galley(
                                pos2(row.right() - 10. - time_galley.size().x, row.top() + 13.),
                                time_galley.clone(),
                                secondary,
                            );
                            let left = row.left() + 61.;
                            let width =
                                (row.right() - 12. - left - time_galley.size().x - 8.).max(30.);
                            text_limit(
                                ui.painter(),
                                &conversation.name,
                                pos2(left, row.top() + 10.),
                                width,
                                1,
                                FontId::new(13., FontFamily::Name("Semibold".into())),
                                foreground,
                            );
                            text_limit(
                                ui.painter(),
                                &conversation.preview,
                                pos2(left, row.top() + 33.),
                                row.right()
                                    - 12.
                                    - left
                                    - self.chat_icon_width(&conversation.phone),
                                2,
                                FontId::proportional(12.),
                                secondary,
                            );
                            if conversation.unread && !selected {
                                ui.painter().circle_filled(
                                    pos2(row.left() + 5., row.center().y),
                                    3.,
                                    BLUE,
                                );
                            }
                            if response.clicked() {
                                self.select(&conversation.phone);
                            }
                            self.chat_icons(ui, row, &conversation.phone, secondary);
                            response.context_menu(|ui| self.chat_actions(ui, &conversation.phone));
                        }
                        if !hits.is_empty() {
                            ui.add_space(6.);
                            ui.label(
                                RichText::new(format!("Сообщения · {hit_count}"))
                                    .small()
                                    .color(SECONDARY),
                            );
                        }
                        for hit in hits {
                            let (row, response) = ui.allocate_exact_size(
                                vec2(rows.width() - 3., layout::ROW_HEIGHT),
                                Sense::click(),
                            );
                            response.widget_info(|| {
                                WidgetInfo::labeled(
                                    Role::Button,
                                    true,
                                    format!("Открыть найденное сообщение {}", hit.message_id),
                                )
                            });
                            if response.hovered()
                                || self.highlighted.as_deref() == Some(&hit.message_id)
                            {
                                ui.painter()
                                    .rect_filled(row, 10, Color32::from_rgb(228, 239, 252));
                            }
                            self.avatar(
                                ui,
                                &hit.phone,
                                pos2(row.left() + 23., row.top() + 23.),
                                15.,
                            );
                            let time = model::time_label(hit.timestamp);
                            let time_width = ui
                                .painter()
                                .layout_no_wrap(time.clone(), FontId::proportional(10.5), SECONDARY)
                                .size()
                                .x;
                            text_limit(
                                ui.painter(),
                                &hit.name,
                                pos2(row.left() + 45., row.top() + 9.),
                                (row.width() - 64. - time_width).max(20.),
                                1,
                                FontId::new(12., FontFamily::Name("Semibold".into())),
                                TEXT,
                            );
                            ui.painter().text(
                                pos2(row.right() - 10., row.top() + 10.),
                                Align2::RIGHT_TOP,
                                &time,
                                FontId::proportional(10.5),
                                SECONDARY,
                            );
                            text_limit(
                                ui.painter(),
                                &hit.text,
                                pos2(row.left() + 10., row.top() + 34.),
                                row.width() - 20.,
                                1,
                                FontId::proportional(12.),
                                SECONDARY,
                            );
                            response
                                .clone()
                                .on_hover_text(format!("{time}\n{}", hit.text));
                            if response.clicked() {
                                self.select(&hit.phone);
                                self.jump_to = Some(hit.message_id.clone());
                                self.highlighted = Some(hit.message_id);
                                self.scroll_to_bottom = false;
                                self.focus_draft = false;
                            }
                        }
                        if hit_count > self.search_limit
                            && ui.button("Показать ещё 100 результатов").clicked()
                        {
                            self.search_limit = self.search_limit.saturating_add(100);
                        }
                        if empty {
                            ui.label(
                                RichText::new(
                                    if self.unread_only && self.search.trim().is_empty() {
                                        "Нет непрочитанных сообщений"
                                    } else if self.search.trim().is_empty() {
                                        "Пока нет диалогов"
                                    } else {
                                        "Ничего не найдено"
                                    },
                                )
                                .size(12.)
                                .color(SECONDARY),
                            );
                        }
                    });
            },
        );
        if !self.preview && !self.connected {
            let bounds = Rect::from_min_size(
                pos2(rect.left() + 10., rect.bottom() - 66.),
                vec2(rect.width() - 20., 28.),
            );
            let response = ui
                .scope_builder(UiBuilder::new().max_rect(bounds), |ui| {
                    ui.add_enabled_ui(!self.connecting, |ui| {
                        ui.add_sized(
                            bounds.size(),
                            Button::new(if self.connecting {
                                "Подключение…"
                            } else {
                                "Подключить"
                            }),
                        )
                    })
                })
                .inner
                .inner;
            if response.clicked() {
                self.connect();
            }
        }
        let footer_y = rect.bottom() - 17.;
        let settings = Rect::from_center_size(pos2(rect.left() + 24., footer_y), vec2(30., 26.));
        let settings_button = icon_button(ui, settings, Icon::Settings, "Настройки · Ctrl+,", true);
        if settings_button.clicked() {
            self.open_settings();
        }
        let status = if self.preview {
            "Предпросмотр · без подключения"
        } else {
            &self.status
        };
        let dot = pos2(settings.right() + 9., footer_y);
        let text_x = dot.x + 8.;
        let right = rect.right() - 10.;
        ui.painter().circle_filled(
            dot,
            2.5,
            if self.connected {
                Color32::from_rgb(52, 199, 89)
            } else {
                Color32::from_rgb(153, 153, 160)
            },
        );
        let mut job = LayoutJob::simple(
            status.to_owned(),
            FontId::proportional(10.5),
            SECONDARY,
            (right - text_x).max(1.),
        );
        job.wrap.max_rows = 1;
        job.wrap.overflow_character = Some('…');
        let galley = ui.painter().layout_job(job);
        ui.painter().galley(
            pos2(text_x, footer_y - galley.rect.center().y),
            galley,
            SECONDARY,
        );
        let hint = if let Some(seconds) = self.reconnect_seconds {
            format!(
                "{}\nАвтоматическая попытка через {seconds} с после последней ошибки. Кнопка «Подключить» запускает её сразу.",
                self.connection_error
                    .as_deref()
                    .unwrap_or("Нет связи с iPhone")
            )
        } else {
            status.to_owned()
        };
        ui.interact(
            Rect::from_min_max(
                pos2(dot.x - 2.5, settings.top()),
                pos2(right, settings.bottom()),
            ),
            Id::new("connection_status"),
            Sense::hover(),
        )
        .on_hover_text(hint);
    }

    fn header(&mut self, ui: &mut Ui, rect: Rect) {
        let compose = Rect::from_min_size(pos2(rect.left() + 10., rect.top() + 8.), vec2(32., 32.));
        if icon_button(ui, compose, Icon::Compose, "Новое сообщение · Ctrl+N", true).clicked()
        {
            self.new_conversation();
        }
        let video = Rect::from_min_size(pos2(rect.right() - 46., rect.top() + 8.), vec2(32., 32.));
        icon_button(ui, video, Icon::Video, "Видеозвонки пока недоступны", false);
        drag_area(
            ui,
            Rect::from_min_max(
                pos2(compose.right() + 5., rect.top()),
                pos2(rect.center().x - 145., rect.bottom()),
            ),
            "header_left",
        );
        drag_area(
            ui,
            Rect::from_min_max(
                pos2(rect.center().x + 145., rect.top()),
                pos2(video.left() - 5., rect.bottom()),
            ),
            "header_right",
        );
        if self.new_mode {
            let input = Rect::from_min_max(
                pos2(compose.right() + 16., rect.top() + 22.),
                pos2(video.left() - 12., rect.top() + 52.),
            );
            ui.painter().text(
                input.left_center(),
                Align2::LEFT_CENTER,
                "Кому:",
                FontId::proportional(14.),
                SECONDARY,
            );
            let response = ui.put(
                Rect::from_min_max(input.min + vec2(50., 4.), input.max),
                TextEdit::singleline(&mut self.recipient)
                    .id(Id::new("recipient"))
                    .frame(Frame::NONE)
                    .hint_text("Номер телефона или email")
                    .font(FontId::proportional(14.)),
            );
            if self.focus_recipient {
                response.request_focus();
                self.focus_recipient = false;
            }
        } else {
            let name = self.name();
            let center = pos2(rect.center().x, rect.top() + 24.);
            let phone = self.selected.clone().unwrap_or_default();
            self.avatar(ui, &phone, center, 15.5);
            let galley = ui.painter().layout_no_wrap(
                name.clone(),
                FontId::new(11., FontFamily::Name("Semibold".into())),
                TEXT,
            );
            let width = (galley.size().x + 36.).min(rect.width() - 150.);
            let capsule =
                Rect::from_center_size(pos2(center.x, rect.top() + 56.), vec2(width, 23.));
            ui.painter()
                .rect_filled(capsule, 12, Color32::from_rgb(244, 244, 246));
            let response = ui.interact(
                Rect::from_min_max(pos2(capsule.left(), rect.top() + 6.), capsule.max),
                Id::new("contact"),
                Sense::click(),
            );
            response
                .widget_info(|| WidgetInfo::labeled(Role::Button, true, "Сведения о собеседнике"));
            text_limit(
                ui.painter(),
                &name,
                pos2(capsule.left() + 10., capsule.top() + 4.),
                (capsule.width() - 32.).max(1.),
                1,
                FontId::new(11., FontFamily::Name("Semibold".into())),
                TEXT,
            );
            paint_icon(
                ui,
                Rect::from_center_size(
                    pos2(capsule.right() - 9., capsule.center().y),
                    vec2(10., 12.),
                ),
                Icon::Chevron,
                SECONDARY,
            );
            if response.clicked() {
                self.prepare_contact();
            }
            Popup::menu(&response)
                .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
                .show(|ui| self.contact_menu(ui));
        }
    }
    fn thread(&mut self, ui: &mut Ui, rect: Rect) {
        let records: Vec<Message> = self
            .messages
            .iter()
            .filter(|m| Some(m.phone.as_str()) == self.selected.as_deref())
            .cloned()
            .collect();
        if records.is_empty() {
            let center = rect.center() - vec2(0., 15.);
            let p = ui.painter();
            paint_icon(
                ui,
                Rect::from_center_size(center - vec2(0., 32.), vec2(52., 48.)),
                Icon::Message,
                Color32::from_rgb(207, 207, 214),
            );
            p.text(
                center + vec2(0., 12.),
                Align2::CENTER_CENTER,
                if self.new_mode {
                    "Новое сообщение"
                } else {
                    "Сообщения"
                },
                FontId::new(21., FontFamily::Name("Semibold".into())),
                TEXT,
            );
            p.text(
                center + vec2(0., 45.),
                Align2::CENTER_CENTER,
                if self.new_mode {
                    "Укажи номер или email и напиши сообщение."
                } else {
                    "Выбери диалог или начни новый."
                },
                FontId::proportional(13.),
                SECONDARY,
            );
            return;
        }
        let jump = self.jump_to.take();
        let force = self.scroll_to_bottom && jump.is_none();
        self.scroll_to_bottom = false;
        ui.scope_builder(
            UiBuilder::new()
                .max_rect(rect)
                .layout(Layout::top_down(Align::Min)),
            |ui| {
                let area = ScrollArea::vertical()
                    .id_salt(("thread", self.selected.clone()))
                    .auto_shrink([false, false])
                    .animated(self.motion.enabled)
                    .stick_to_bottom(jump.is_none());
                area.show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = layout::MESSAGE_GAP;
                    ui.set_width(rect.width() - 6.);
                    let width = ui.available_width();
                    let max_width = (width * 0.73).min(450.);
                    let mut previous = 0i64;
                    for (index, message) in records.iter().enumerate() {
                        if message.timestamp - previous > 20 * 60 {
                            ui.add_space(8.);
                            let time = Local
                                .timestamp_opt(message.timestamp, 0)
                                .single()
                                .unwrap_or_else(Local::now);
                            let label = format!(
                                "{} {}",
                                if time.date_naive() == Local::now().date_naive() {
                                    "Сегодня".into()
                                } else {
                                    time.format("%d.%m.%Y").to_string()
                                },
                                time.format("%H:%M")
                            );
                            ui.allocate_ui_with_layout(
                                vec2(width, 16.),
                                Layout::top_down(Align::Center),
                                |ui| {
                                    ui.label(RichText::new(label).size(10.5).color(SECONDARY));
                                },
                            );
                            ui.add_space(10.);
                        }
                        let foreground = if message.outgoing {
                            Color32::WHITE
                        } else {
                            TEXT
                        };
                        let job = LayoutJob::simple(
                            crate::media::display(&message.text).to_owned(),
                            FontId::proportional(14.),
                            foreground,
                            max_width - 24.,
                        );
                        let galley = ui.painter().layout_job(job);
                        let size = layout::bubble_size(galley.size());
                        let (row, _) = ui.allocate_exact_size(vec2(width, size.y), Sense::hover());
                        let bubble = Rect::from_min_size(
                            pos2(
                                if message.outgoing {
                                    row.right() - size.x - 4.
                                } else {
                                    row.left() + 4.
                                },
                                row.top(),
                            ),
                            size,
                        );
                        let fill = if message.outgoing {
                            BLUE
                        } else {
                            Color32::from_rgb(233, 233, 235)
                        };
                        let last = index + 1 == records.len()
                            || records[index + 1].outgoing != message.outgoing
                            || records[index + 1].timestamp - message.timestamp > 300;
                        let (progress, opacity, active) = self.motion.message(&message.id);
                        if active {
                            ui.ctx().request_repaint();
                        }
                        let scale = 0.96 + 0.04 * progress;
                        let origin = if message.outgoing {
                            bubble.right_bottom()
                        } else {
                            bubble.left_bottom()
                        };
                        let transform = emath::TSTransform::new(
                            origin.to_vec2() * (1. - scale) + vec2(0., 8. * (1. - progress)),
                            scale,
                        );
                        let mut bubble_ui = ui.new_child(
                            UiBuilder::new()
                                .id_salt((&message.id, "bubble"))
                                .max_rect(bubble),
                        );
                        bubble_ui.with_visual_transform(transform, |ui| {
                            ui.multiply_opacity(opacity);
                            if last {
                                let x = if message.outgoing {
                                    bubble.right() - 4.
                                } else {
                                    bubble.left() + 4.
                                };
                                let y = bubble.bottom();
                                let dir = if message.outgoing { 1. } else { -1. };
                                ui.painter().add(Shape::convex_polygon(
                                    vec![
                                        pos2(x, y - 13.),
                                        pos2(x + dir * 8., y + 1.),
                                        pos2(x - dir * 6., y - 3.),
                                    ],
                                    fill,
                                    Stroke::NONE,
                                ));
                            }
                            ui.painter().rect_filled(bubble, 17, fill);
                            if self.highlighted.as_deref()==Some(&message.id) {
                                ui.painter().rect_stroke(bubble.expand(3.),20,Stroke::new(2.,Color32::from_rgb(255,183,46)),StrokeKind::Outside);
                            }
                            let response = ui.place(
                                bubble.shrink2(layout::BUBBLE_PADDING),
                                Label::new(galley.clone())
                                    .wrap()
                                    .halign(Align::Min)
                                    .selectable(true),
                            );
                            response.context_menu(|ui| {
                                if ui.button("Копировать").clicked() {
                                    ui.ctx().copy_text(message.text.clone());
                                    ui.close();
                                }
                                if matches!(message.state,SendState::Failed|SendState::Unconfirmed)
                                    && ui.add_enabled(!self.sending,Button::new("Повторить отправку…")).clicked() {
                                    self.retry=Some(message.clone());
                                    ui.close();
                                }
                            });
                        });
                        if jump.as_deref()==Some(&message.id) {
                            ui.scroll_to_rect(bubble,Some(Align::Center));
                        }
                        if last {
                            ui.add_space(layout::GROUP_GAP - layout::MESSAGE_GAP);
                        }
                        if message.outgoing && (index + 1 == records.len() || matches!(message.state,SendState::Failed|SendState::Unconfirmed|SendState::Pending)) {
                            let state = match message.state {
                                SendState::Pending => "Отправляется…",
                                SendState::Accepted => "Передано iPhone",
                                SendState::Failed => "Не отправлено",
                                SendState::Unconfirmed => "Результат отправки неизвестен",
                                _ => "",
                            };
                            ui.allocate_ui_with_layout(
                                vec2(width, 23.),
                                Layout::right_to_left(Align::Center),
                                |ui| {
                                    if matches!(message.state,SendState::Failed|SendState::Unconfirmed)
                                        && ui.add_enabled(!self.sending,Button::new(RichText::new("Повторить…").size(10.))).clicked() {
                                        self.retry=Some(message.clone());
                                    }
                                    ui.label(RichText::new(state).size(10.).color(if matches!(message.state,SendState::Failed|SendState::Unconfirmed) {Color32::from_rgb(176,58,58)} else {SECONDARY}))
                                        .on_hover_text("«Передано iPhone» означает, что телефон принял сообщение. Это не подтверждение доставки адресату.");
                                },
                            );
                        }
                        previous = message.timestamp;
                    }
                    ui.add_space(15.);
                    if force {
                        ui.scroll_to_cursor(Some(Align::BOTTOM));
                    }
                });
            },
        );
    }

    fn composer(&mut self, ui: &mut Ui, rect: Rect) {
        let input = layout::composer_input(rect);
        ui.painter()
            .rect_filled(input, 18, Color32::from_rgb(251, 251, 253));
        ui.painter().rect_stroke(
            input,
            18,
            Stroke::new(1.0_f32, Color32::from_rgb(218, 218, 224)),
            StrokeKind::Inside,
        );
        let plus = icon_button(
            ui,
            Rect::from_center_size(
                pos2(rect.left() + 30., input.bottom() - 20.),
                vec2(30., 30.),
            ),
            Icon::Plus,
            "Действия",
            true,
        );
        if plus.clicked() {
            self.prepare_contact();
        }
        Popup::menu(&plus)
            .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                if ui.button("Новое сообщение").clicked() {
                    self.new_conversation();
                    ui.close();
                }
                ui.add_enabled(false, Button::new("Фото недоступны через Bluetooth"))
                    .on_hover_text("iPhone не передаёт изображения через этот канал сообщений.");
                ui.separator();
                self.contact_menu(ui);
            });
        if self.focus_draft {
            ui.ctx()
                .memory_mut(|memory| memory.request_focus(Id::unique("draft")));
            self.focus_draft = false;
        }
        if crate::composer::edit(ui, &mut self.draft, input, true)
            .response
            .changed()
        {
            self.error = None;
        }
        let available = !self.preview
            && !self.sending
            && !self.draft.trim().is_empty()
            && (self.selected.is_some() || !self.recipient.trim().is_empty());
        let send = icon_button(
            ui,
            Rect::from_center_size(
                pos2(input.right() - 20., input.bottom() - 20.),
                vec2(27., 27.),
            ),
            Icon::Arrow,
            "Отправить · Enter (Shift+Enter — новая строка)",
            available,
        );
        if send.clicked() {
            self.send();
        }
        let emoji = icon_button(
            ui,
            Rect::from_center_size(
                pos2(rect.right() - 30., input.bottom() - 20.),
                vec2(30., 30.),
            ),
            Icon::Smile,
            "Эмодзи",
            true,
        );
        Popup::menu(&emoji)
            .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                ui.set_width(348.);
                ui.label(RichText::new("Эмодзи Apple").strong().size(14.));
                ui.add(
                    TextEdit::singleline(&mut self.emoji_query)
                        .hint_text("Поиск эмодзи")
                        .desired_width(f32::INFINITY),
                );
                ComboBox::from_id_salt("emoji_category")
                    .selected_text(if self.emoji_category.is_empty() {
                        "Все категории"
                    } else {
                        emoji::category_name(&self.emoji_category)
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.emoji_category,
                            String::new(),
                            "Все категории",
                        );
                        for category in [
                            "Smileys & Emotion",
                            "People & Body",
                            "Animals & Nature",
                            "Food & Drink",
                            "Travel & Places",
                            "Activities",
                            "Objects",
                            "Symbols",
                            "Flags",
                        ] {
                            ui.selectable_value(
                                &mut self.emoji_category,
                                category.to_owned(),
                                emoji::category_name(category),
                            );
                        }
                    });
                if let Some(catalog) = &self.emoji_catalog {
                    let query = self.emoji_query.trim().to_lowercase();
                    let entries: Vec<_> = catalog
                        .entries
                        .iter()
                        .filter(|e| {
                            (self.emoji_category.is_empty() || self.emoji_category == e.category)
                                && (query.is_empty()
                                    || e.search.contains(&query)
                                    || e.text.contains(&query))
                        })
                        .collect();
                    ui.label(
                        RichText::new(format!("{} эмодзи и вариантов", entries.len()))
                            .size(11.)
                            .color(SECONDARY),
                    );
                    let mut chosen = None;
                    ScrollArea::vertical()
                        .max_height(270.)
                        .id_salt("emoji_grid")
                        .show_rows(ui, 36., entries.len().div_ceil(9), |ui, rows| {
                            for row in rows {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 2.;
                                    for entry in entries.iter().skip(row * 9).take(9) {
                                        if ui
                                            .add_sized(
                                                [35., 34.],
                                                Button::new(RichText::new(&entry.text).size(23.))
                                                    .frame(false),
                                            )
                                            .on_hover_text(&entry.name)
                                            .clicked()
                                        {
                                            chosen = Some(entry.text.clone());
                                        }
                                    }
                                });
                            }
                        });
                    if let Some(chosen) = chosen {
                        self.draft.push_str(&chosen);
                    }
                } else {
                    ui.label("Не найден каталог в assets/emoji.");
                }
            });
    }

    fn seed_preview(&mut self) {
        for (c, name) in ["Анна", "Александр", "Мария", "Команда"].iter().enumerate()
        {
            let texts: Vec<&str> = if c == 0 {
                vec![
                    "Привет! Как проходит твой день?",
                    "Привет! Всё отлично. Теперь отвечаю прямо с ноутбука 😊",
                    "Получилось подключить сообщения?",
                    "Да! Теперь всё под рукой.",
                    "Красота. Тогда до встречи вечером ❤️",
                ]
            } else {
                vec![match c {
                    1 => "Отправил тебе все материалы",
                    2 => "Спасибо! До завтра",
                    _ => "Созвонимся в понедельник",
                }]
            };
            let base = Local::now().timestamp() - c as i64 * 900 - 300;
            for (i, text) in texts.iter().enumerate() {
                self.messages.push(Message {
                    id: format!("preview:{c}:{i}"),
                    handle: String::new(),
                    phone: format!("+7999000000{c}"),
                    name: (*name).into(),
                    text: (*text).into(),
                    timestamp: base + i as i64 * 60,
                    outgoing: c == 0 && (i == 1 || i == 3),
                    read: true,
                    state: SendState::Received,
                });
            }
        }
        self.rebuild_conversations();
        self.selected = self.conversations.first().map(|c| c.phone.clone());
    }
}

impl eframe::App for MessagesApp {
    fn clear_color(&self, _: &Visuals) -> [f32; 4] {
        [1.; 4]
    }
    fn logic(&mut self, ctx: &Context, _: &mut eframe::Frame) {
        self.receive(ctx);
    }
    fn ui(&mut self, root_ui: &mut Ui, _: &mut eframe::Frame) {
        self.render(root_ui);
    }
}
impl MessagesApp {
    fn render(&mut self, root_ui: &mut Ui) {
        let ctx = root_ui.ctx().clone();
        if !self.settings_open
            && !self.new_mode
            && self.retry.is_none()
            && !self.hidden
            && ctx.input(|input| input.focused && input.viewport().minimized != Some(true))
        {
            self.mark_selected_read();
        }
        self.update_shell(&ctx);
        if let Ok(mut active) = self.shared_active.lock() {
            *active = if !self.hidden
                && !self.settings_open
                && !self.new_mode
                && self.retry.is_none()
                && ctx.input(|input| input.focused && input.viewport().minimized != Some(true))
            {
                self.selected.clone()
            } else {
                None
            };
        }
        if self.preferences.auto_focus
            && !self.settings_open
            && !Popup::is_any_open(&ctx)
            && self.selected.is_some()
            && ctx.input(|i| {
                i.events
                    .iter()
                    .any(|event| matches!(event, eframe::egui::Event::WindowFocused(true)))
            })
        {
            self.focus_draft = true;
        }
        if ctx.memory(|m| m.has_focus(Id::unique("draft")))
            && ctx.input_mut(|i| {
                let mut found = false;
                i.events.retain(|event| {
                    if matches!(event, eframe::egui::Event::PasteImage(_)) {
                        found = true;
                        false
                    } else {
                        true
                    }
                });
                found
            })
        {
            self.error = Some(crate::media::UNAVAILABLE.into());
        }
        self.handle_lifecycle(&ctx);
        if ctx.input_mut(|i| i.consume_key(Modifiers::CTRL, Key::Comma)) {
            self.open_settings();
        }
        if self.settings_open && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            self.settings_open = false;
            self.focus_draft = true;
        }
        if !self.settings_open
            && self.retry.is_none()
            && !self.search.is_empty()
            && !Popup::is_any_open(&ctx)
            && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape))
        {
            self.search.clear();
            self.highlighted = None;
            self.focus_draft = true;
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::CTRL, Key::N)) {
            self.new_conversation();
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::CTRL, Key::F)) {
            self.focus_search = true;
        }
        let enter = self.retry.is_none()
            && ctx.memory(|m| m.has_focus(Id::new("draft")))
            && !ctx.input(|i| i.modifiers.shift)
            && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));
        CentralPanel::default()
            .frame(Frame::NONE)
            .show(root_ui, |ui| {
                let bounds = ui.max_rect();
                ui.painter().rect_filled(bounds, 0, Color32::WHITE);
                let measured_main_width = layout::Regions::new(bounds, 40., false).main.width();
                let draft_width = layout::composer_text_width(measured_main_width);
                let measured = ui.painter().layout(
                    self.draft.clone(),
                    FontId::proportional(14.),
                    TEXT,
                    draft_width,
                );
                let input_height = (measured.size().y + 20.).clamp(40., 128.);
                let (input_height, active) = self.motion.composer.step(
                    input_height,
                    ctx.input(|i| i.stable_dt),
                    self.motion.enabled,
                );
                if active {
                    ctx.request_repaint();
                }
                let shown_error = self
                    .error
                    .as_ref()
                    .or(self.connection_error.as_ref())
                    .or(self.notification_error.as_ref())
                    .cloned();
                let regions = layout::Regions::new(bounds, input_height, shown_error.is_some());
                self.sidebar(ui, regions.sidebar);
                if self.settings_open {
                    self.settings_panel(ui, regions.main);
                    resize_edges(ui, bounds);
                    return;
                }
                self.header(ui, regions.header);
                let (opacity, active) = self.motion.dialogue_opacity();
                ui.scope(|ui| {
                    ui.multiply_opacity(opacity);
                    self.thread(ui, regions.thread);
                });
                if active {
                    ctx.request_repaint();
                }
                if let Some(error) = &shown_error {
                    text_limit(
                        ui.painter(),
                        error,
                        regions.error.min + vec2(0., 4.),
                        regions.error.width(),
                        2,
                        FontId::proportional(11.),
                        Color32::from_rgb(176, 58, 58),
                    );
                }
                self.composer(ui, regions.composer);
                resize_edges(ui, bounds);
            });
        self.retry_dialog(&ctx);
        if enter && !self.settings_open {
            self.send();
        }
        if self.focus_draft {
            ctx.request_repaint();
        }
        // No constant render loop: only input, worker events, and native text-caret blinking repaint.
        ctx.request_repaint_after(Duration::from_secs(12));
    }
}
impl Drop for MessagesApp {
    fn drop(&mut self) {
        if !self.preview && self.preferences.save_drafts {
            let mut messages = self.drafts.clone();
            if let Some(phone) = &self.selected {
                messages.insert(phone.clone(), self.draft.clone());
            } else if self.new_mode {
                messages.insert(String::new(), self.draft.clone());
            }
            messages.retain(|_, text| !text.is_empty());
            let saved = crate::settings::Drafts {
                messages,
                selected: self.selected.clone(),
                new_mode: self.new_mode,
                recipient: self.recipient.clone(),
                uncertain_submission: self.sending,
                submitted: self.submitted.clone(),
            };
            let _ = saved.save(&model::data_dir().join("drafts.bin"));
        }
        if let Some(commands) = &self.commands {
            let _ = commands.send(Command::Stop);
        }
    }
}

impl MessagesApp {
    fn open_settings(&mut self) {
        self.settings_draft = self.preferences.clone();
        self.settings_open = true;
        self.focus_draft = false;
    }
    fn apply_settings(&mut self, ctx: &Context) -> bool {
        self.settings_draft.normalize();
        self.settings_draft.chats = self.preferences.chats.clone();
        if !self.preview {
            let autostart_changed = self.settings_draft.autostart != self.preferences.autostart;
            if autostart_changed
                && let Err(error) = crate::desktop::set_autostart(self.settings_draft.autostart)
            {
                self.settings_status = format!("Не удалось изменить автозапуск: {error}");
                return false;
            }
            if let Err(error) = self
                .settings_draft
                .save(&model::data_dir().join("settings.bin"))
            {
                if autostart_changed {
                    let _ = crate::desktop::set_autostart(self.preferences.autostart);
                }
                self.settings_status = format!("Не удалось сохранить настройки: {error}");
                return false;
            }
            if !self.settings_draft.save_drafts {
                let path = model::data_dir().join("drafts.bin");
                if path.exists()
                    && let Err(error) = fs::remove_file(path)
                {
                    self.settings_status =
                        format!("Не удалось удалить сохранённые черновики: {error}");
                    return false;
                }
            }
        }
        self.preferences = self.settings_draft.clone();
        if let Ok(mut shared) = self.shared_preferences.lock() {
            *shared = self.preferences.clone();
        }
        self.motion.enabled = crate::motion::Motion::new().enabled && self.preferences.animations;
        ctx.style_mut_of(Theme::Light, |style| {
            style.animation_time = if self.motion.enabled {
                crate::motion::POPUP_TIME
            } else {
                0.
            }
        });
        self.settings_status = "Настройки сохранены".into();
        if let Some(commands) = &self.commands {
            let _ = commands.send(Command::Refresh);
        }
        true
    }
    fn settings_panel(&mut self, ui: &mut Ui, rect: Rect) {
        drag_area(
            ui,
            Rect::from_min_size(rect.min, vec2(rect.width(), 42.)),
            "settings_drag",
        );
        ui.scope_builder(UiBuilder::new().max_rect(rect.shrink2(vec2(28.,20.))).layout(Layout::top_down(Align::Min)),|ui|{
            ui.heading(RichText::new("Настройки").family(FontFamily::Name("Semibold".into())).size(23.));
            ui.add_space(12.);
            let available=(ui.available_height()-105.).max(150.);
            ScrollArea::vertical().max_height(available).show(ui,|ui|{
                ui.set_max_width(ui.available_width().min(610.));
                ui.spacing_mut().item_spacing.y=9.;
                ui.label(RichText::new("Поведение").strong());
                ui.checkbox(&mut self.settings_draft.auto_focus,"Сразу печатать при возврате в приложение");
                ui.checkbox(&mut self.settings_draft.animations,"Плавные анимации");
                ui.checkbox(&mut self.settings_draft.minimize_on_close,"Скрывать в трей при закрытии окна");
                ui.small("Сообщения продолжают приходить. Полный выход — через меню значка в трее.");
                if !self.desktop.as_ref().is_some_and(|tray|tray.available()) {
                    ui.small(if self.preview {"Предпросмотр без трея"} else {"Значок в трее недоступен: закрытие окна завершит приложение."});
                }
                ui.checkbox(&mut self.settings_draft.autostart,"Запускать с Windows в трее");
                ui.add_space(9.);ui.separator();ui.add_space(9.);
                ui.label(RichText::new("Уведомления").strong());
                ui.checkbox(&mut self.settings_draft.notifications,"Показывать новые сообщения в Windows");
                ui.add_enabled_ui(self.settings_draft.notifications,|ui|{ui.checkbox(&mut self.settings_draft.notification_sound,"Звук уведомлений");});
                ui.add_enabled_ui(self.settings_draft.notifications,|ui|{ui.checkbox(&mut self.settings_draft.notification_preview,"Показывать текст сообщения в уведомлениях");});
                ui.small("Если текст скрыт, уведомление содержит только имя и «Новое сообщение».");
                ui.horizontal(|ui|{ui.label("Проверять сообщения каждые");ui.add(DragValue::new(&mut self.settings_draft.poll_seconds).range(2..=60).suffix(" с"));});
                ui.add_space(9.);ui.separator();ui.add_space(9.);
                ui.label(RichText::new("История и черновики").strong());
                ui.checkbox(&mut self.settings_draft.save_history,"Сохранять сообщения на этом компьютере");
                ui.checkbox(&mut self.settings_draft.sync_history_on_connect,"Загружать доступные старые сообщения при подключении");
                ui.checkbox(&mut self.settings_draft.save_drafts,"Восстанавливать черновики после перезапуска");
                ui.small("Данные зашифрованы для твоей учётной записи Windows. Отключение истории останавливает запись; сохранённый архив остаётся.");
                ui.add_space(9.);ui.separator();ui.add_space(9.);
                ui.label(RichText::new("Подключение к iPhone").strong());
                ui.label("Способ отправки: автоматически на iPhone");
                ui.small("Bluetooth не предоставляет подтверждённого выбора iMessage / SMS. Фотографии и полный архив переписки в текущем канале недоступны.");
                ui.small(format!("Сохранено сообщений: {}",self.messages.len()));
                if ui.button("Обновить доступные сообщения").clicked() && let Some(commands)=&self.commands{let _=commands.send(Command::SyncHistory);}
                ui.add_space(15.);
                ui.small(format!("meMessage {}",env!("CARGO_PKG_VERSION")));
            });
                ui.horizontal(|ui|{
                    ui.spacing_mut().item_spacing.x=4.;
                    ui.label(RichText::new("Сделано by").size(12.).color(SECONDARY));
                    let font=FontId::new(12.5,FontFamily::Name("Semibold".into()));
                    let width=ui.painter().layout_no_wrap("MAARK".into(),font.clone(),BLUE).size().x;
                    let response=ui.allocate_response(vec2(width,25.),Sense::click()).on_hover_cursor(CursorIcon::PointingHand).on_hover_text("Открыть maark.ru");
                    response.widget_info(||WidgetInfo::labeled(Role::Link,true,"MAARK"));
                    let t=ui.ctx().animate_bool_with_time(response.id.with("hover"),response.hovered(),if self.motion.enabled{0.16}else{0.});
                    let color=Color32::from_rgb((7.+65.*t) as u8,(131.-37.*t) as u8,255);
                    let center=response.rect.center()-vec2(0.,1.5*t);
                    ui.painter().text(center,Align2::CENTER_CENTER,"MAARK",font,color);
                    if t>0.001{ui.painter().line_segment([pos2(center.x-width*0.5*t,response.rect.bottom()-3.),pos2(center.x+width*0.5*t,response.rect.bottom()-3.)],Stroke::new(1.,color.linear_multiply(t)));}
                    if response.clicked(){ui.ctx().open_url(OpenUrl::new_tab("https://maark.ru"));}
                });
            ui.add_space(6.);
            ui.horizontal(|ui|{
                if ui.button("Сохранить").clicked(){self.apply_settings(ui.ctx());}
                if ui.button("Вернуться к сообщениям").clicked(){self.settings_open=false;self.focus_draft=true;}
            });
            if !self.settings_status.is_empty(){ui.add(Label::new(RichText::new(&self.settings_status).small()).truncate()).on_hover_text(&self.settings_status);}
        });
    }
}

fn text_limit(
    p: &Painter,
    text: &str,
    pos: Pos2,
    width: f32,
    rows: usize,
    font: FontId,
    color: Color32,
) {
    let mut job = LayoutJob::simple(text.into(), font, color, width.max(1.));
    job.wrap = TextWrapping {
        max_width: width.max(1.),
        max_rows: rows,
        break_anywhere: false,
        overflow_character: Some('…'),
    };
    let galley = p.layout_job(job);
    p.galley(pos, galley, color);
}
fn icon_button(ui: &mut Ui, rect: Rect, icon: Icon, label: &str, enabled: bool) -> Response {
    let response = ui.interact(
        rect,
        Id::new(label),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    response.widget_info(|| WidgetInfo::labeled(Role::Button, enabled, label));
    if enabled && response.has_focus() {
        ui.painter().rect_stroke(
            rect.expand(1.),
            5,
            Stroke::new(1.5, BLUE),
            StrokeKind::Outside,
        );
    }
    let color = if enabled {
        Color32::from_rgb(85, 85, 93)
    } else {
        Color32::from_rgb(184, 184, 191)
    };
    if matches!(icon, Icon::Arrow) {
        ui.painter().circle_filled(
            rect.center(),
            rect.width() / 2.,
            if enabled {
                BLUE
            } else {
                Color32::from_rgb(197, 197, 205)
            },
        );
    } else if response.hovered() && enabled {
        ui.painter().rect_filled(
            rect,
            16,
            Color32::from_black_alpha(if response.is_pointer_button_down_on() {
                24
            } else {
                12
            }),
        );
    }
    paint_icon(
        ui,
        rect.shrink(match icon {
            Icon::Arrow => 7.,
            Icon::Settings => 4.,
            _ => 7.5,
        }),
        icon,
        if matches!(icon, Icon::Arrow) {
            Color32::WHITE
        } else {
            color
        },
    );
    response
        .on_hover_cursor(if enabled {
            CursorIcon::PointingHand
        } else {
            CursorIcon::Default
        })
        .on_hover_text(label)
}
fn drag_area(ui: &mut Ui, rect: Rect, id: &str) {
    if rect.width() < 1. {
        return;
    }
    let response = ui.interact(rect, Id::new(id), Sense::click_and_drag());
    if response.drag_started() {
        ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
    }
    if response.double_clicked() {
        let max = ui.input(|i| i.viewport().maximized.unwrap_or(false));
        ui.ctx().send_viewport_cmd(ViewportCommand::Maximized(!max));
    }
}
fn resize_edges(ui: &mut Ui, bounds: Rect) {
    if ui.input(|i| i.viewport().maximized.unwrap_or(false)) {
        return;
    }
    let Some(pos) = ui.input(|i| i.pointer.hover_pos()) else {
        return;
    };
    let pad = 5.;
    let left = pos.x < bounds.left() + pad;
    let right = pos.x > bounds.right() - pad;
    let top = pos.y < bounds.top() + pad;
    let bottom = pos.y > bounds.bottom() - pad;
    let (direction, cursor) = match (left, right, top, bottom) {
        (true, _, true, _) => (viewport::ResizeDirection::NorthWest, CursorIcon::ResizeNwSe),
        (_, true, true, _) => (viewport::ResizeDirection::NorthEast, CursorIcon::ResizeNeSw),
        (true, _, _, true) => (viewport::ResizeDirection::SouthWest, CursorIcon::ResizeNeSw),
        (_, true, _, true) => (viewport::ResizeDirection::SouthEast, CursorIcon::ResizeNwSe),
        (true, _, _, _) => (
            viewport::ResizeDirection::West,
            CursorIcon::ResizeHorizontal,
        ),
        (_, true, _, _) => (
            viewport::ResizeDirection::East,
            CursorIcon::ResizeHorizontal,
        ),
        (_, _, true, _) => (viewport::ResizeDirection::North, CursorIcon::ResizeVertical),
        (_, _, _, true) => (viewport::ResizeDirection::South, CursorIcon::ResizeVertical),
        _ => return,
    };
    ui.ctx().set_cursor_icon(cursor);
    if ui.input(|i| i.pointer.primary_pressed()) {
        ui.ctx()
            .send_viewport_cmd(ViewportCommand::BeginResize(direction));
    }
}

pub fn app_icon() -> IconData {
    let mut rgba = vec![0; 64 * 64 * 4];
    for y in 0..64 {
        for x in 0..64 {
            let round = |x: f32, y: f32, cx: f32, cy: f32, r: f32| {
                ((x - cx).max(0.)).hypot((y - cy).max(0.)) <= r
            };
            let inside = round(
                (x as f32 - 31.5).abs(),
                (y as f32 - 31.5).abs(),
                20.,
                20.,
                11.5,
            );
            if !inside {
                continue;
            }
            let mut color = [43, 205, 83, 255];
            let dx = (x as f32 - 32.) / 23.;
            let dy = (y as f32 - 29.) / 18.;
            if dx * dx + dy * dy <= 1.
                || ((19..31).contains(&x) && (43..51).contains(&y) && x + y < 71)
            {
                color = [255, 255, 255, 255];
            }
            let i = (y * 64 + x) * 4;
            rgba[i..i + 4].copy_from_slice(&color);
        }
    }
    IconData {
        rgba,
        width: 64,
        height: 64,
    }
}

#[cfg(test)]
mod scenario_tests {
    use super::*;
    use eframe::egui::Event as Input;
    struct Harness {
        ctx: Context,
        app: MessagesApp,
        labels: HashMap<String, Rect>,
        time: f64,
        size: Vec2,
        bubbles: Vec<Rect>,
        opened_urls: Vec<String>,
        draft_clip: Option<Rect>,
        elided_texts: Vec<String>,
        focused: bool,
        text_rects: HashMap<String, Rect>,
        highlighted_bubble: Option<Rect>,
    }
    impl Drop for Harness {
        fn drop(&mut self) {
            // Even a panicking scenario must never persist a synthetic draft.
            self.app.preview = true;
            self.app.commands = None;
        }
    }
    impl Harness {
        fn new(size: Vec2) -> Self {
            let ctx = Context::default();
            ctx.enable_accesskit();
            let mut app = MessagesApp::from_context(&ctx, true);
            app.motion.enabled = false;
            let mut harness = Self {
                ctx,
                app,
                labels: HashMap::new(),
                time: 0.,
                size,
                bubbles: Vec::new(),
                opened_urls: Vec::new(),
                draft_clip: None,
                elided_texts: Vec::new(),
                focused: true,
                text_rects: HashMap::new(),
                highlighted_bubble: None,
            };
            harness.frame(vec![]);
            harness.frame(vec![]);
            harness
        }
        fn frame(&mut self, events: Vec<Input>) {
            for event in &events {
                if let Input::WindowFocused(focused) = event {
                    self.focused = *focused;
                }
            }
            self.time += 1. / 60.;
            self.app.receive(&self.ctx);
            let mut output = self.ctx.run_ui(
                RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, self.size)),
                    time: Some(self.time),
                    focused: self.focused,
                    events,
                    ..Default::default()
                },
                |ui| self.app.render(ui),
            );
            self.labels.clear();
            self.text_rects = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    epaint::Shape::Text(text) => Some((
                        text.galley.job.text.clone(),
                        text.galley.rect.translate(text.pos.to_vec2()),
                    )),
                    _ => None,
                })
                .collect();
            self.elided_texts = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    epaint::Shape::Text(text) if text.galley.elided => {
                        Some(text.galley.job.text.clone())
                    }
                    _ => None,
                })
                .collect();
            self.draft_clip = output.shapes.iter().find_map(|shape| match &shape.shape {
                epaint::Shape::Text(text)
                    if !self.app.draft.is_empty() && text.galley.job.text == self.app.draft =>
                {
                    Some(shape.clip_rect)
                }
                _ => None,
            });
            for command in &output.platform_output.commands {
                if let OutputCommand::OpenUrl(url) = command {
                    self.opened_urls.push(url.url.clone());
                }
            }
            self.bubbles = output
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    epaint::Shape::Rect(shape)
                        if shape.corner_radius.nw == 17
                            && (shape.fill == BLUE
                                || shape.fill == Color32::from_rgb(233, 233, 235)) =>
                    {
                        Some(shape.rect)
                    }
                    _ => None,
                })
                .collect();
            self.highlighted_bubble = output.shapes.iter().find_map(|shape| match &shape.shape {
                epaint::Shape::Rect(rect)
                    if rect.stroke.color == Color32::from_rgb(255, 183, 46) =>
                {
                    Some(rect.rect)
                }
                _ => None,
            });
            if let Some(update) = &output.platform_output.accesskit_update {
                for (_, node) in &update.nodes {
                    if let Some(label) = node.label().or_else(|| node.value()) {
                        let rect = node
                            .bounds()
                            .map(|bounds| {
                                Rect::from_min_max(
                                    pos2(bounds.x0 as f32, bounds.y0 as f32),
                                    pos2(bounds.x1 as f32, bounds.y1 as f32),
                                )
                            })
                            .unwrap_or(Rect::NOTHING);
                        self.labels.insert(label.to_owned(), rect);
                    }
                }
            }
            output.textures_delta.clear();
        }
        fn click(&mut self, label: &str) {
            let rect = *self.labels.get(label).unwrap_or_else(|| {
                panic!(
                    "Missing control {label:?}; labels: {:?}",
                    self.labels.keys().collect::<Vec<_>>()
                )
            });
            let pos = rect.center();
            self.frame(vec![
                Input::PointerMoved(pos),
                Input::PointerButton {
                    pos,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                },
            ]);
            self.frame(vec![Input::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            }]);
            self.frame(vec![]);
        }
        fn key(&mut self, key: Key, modifiers: Modifiers) {
            self.frame(vec![
                Input::ModifiersChanged(modifiers),
                Input::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers,
                },
            ]);
            self.frame(vec![
                Input::Key {
                    key,
                    physical_key: None,
                    pressed: false,
                    repeat: false,
                    modifiers,
                },
                Input::ModifiersChanged(Modifiers::NONE),
            ]);
        }
    }
    #[test]
    fn emoji_panel_supports_multiple_choices_and_escape_at_both_window_sizes() {
        for size in [vec2(830., 570.), vec2(1120., 760.)] {
            let mut h = Harness::new(size);
            h.click("Эмодзи");
            assert!(
                h.labels.contains_key("Эмодзи Apple"),
                "opening popup: {:?}",
                h.labels
            );
            h.click("😀");
            h.click("😃");
            h.click("😀");
            assert_eq!(h.app.draft, "😀😃😀");
            assert!(
                h.labels.contains_key("Эмодзи Apple"),
                "multiple selection: {:?}",
                h.labels
            );
            h.key(Key::Escape, Modifiers::NONE);
            assert!(!h.labels.contains_key("Эмодзи Apple"));
            assert_eq!(h.app.draft, "😀😃😀");
        }
    }
    #[test]
    fn drafts_survive_conversation_changes_and_contact_alias_survives_snapshots() {
        let mut h = Harness::new(vec2(830., 570.));
        let first = h.app.selected.clone().unwrap();
        let other = h
            .app
            .conversations
            .iter()
            .find(|c| c.phone != first)
            .unwrap()
            .phone
            .clone();
        h.app.draft = "Привет ❤️ 👍🏽".into();
        h.app.select(&other);
        h.app.draft = "Другой черновик".into();
        h.app.select(&first);
        assert_eq!(h.app.draft, "Привет ❤️ 👍🏽");
        assert!(h.app.save_contact(&first, "Любимая ❤️"));
        let (tx, rx) = std::sync::mpsc::channel();
        h.app.events = Some(rx);
        tx.send(Event::Contacts(Book::default())).unwrap();
        tx.send(Event::Snapshot(h.app.messages.clone())).unwrap();
        h.frame(vec![]);
        assert_eq!(h.app.name(), "Любимая ❤️");
        h.app.select(&other);
        assert_eq!(h.app.draft, "Другой черновик");
    }
    #[test]
    fn failed_send_preserves_draft_and_polling_never_erases_its_warning_or_resends() {
        let mut h = Harness::new(vec2(1120., 760.));
        let (commands, rx) = std::sync::mpsc::channel();
        let (events, ui_events) = std::sync::mpsc::channel();
        h.app.preview = false;
        h.app.commands = Some(commands);
        h.app.events = Some(ui_events);
        h.app.draft = "Сообщение только тестовому транспорту 😊".into();
        h.app.send();
        assert!(matches!(rx.try_recv(), Ok(Command::Send { .. })));
        assert!(h.app.sending);
        events
            .send(Event::SendFailed("Проверь отправку на iPhone".into()))
            .unwrap();
        events
            .send(Event::Connection(true, "Подключён".into()))
            .unwrap();
        h.frame(vec![]);
        assert!(!h.app.sending);
        assert_eq!(h.app.error.as_deref(), Some("Проверь отправку на iPhone"));
        assert!(h.app.draft.contains("тестовому"));
        assert!(rx.try_recv().is_err());
        h.app.preview = true;
    }
    #[test]
    fn invalid_phone_blocks_send_and_success_clears_the_matching_draft() {
        let mut h = Harness::new(vec2(830., 570.));
        let (commands, rx) = std::sync::mpsc::channel();
        h.app.preview = false;
        h.app.commands = Some(commands);
        h.app.new_conversation();
        h.app.recipient = "не номер".into();
        h.app.draft = "Тест".into();
        h.app.send();
        assert!(h.app.error.is_some());
        assert!(!h.app.sending);
        assert!(rx.try_recv().is_err());
        h.app.recipient = "+7 (999) 123-45-67".into();
        h.app.send();
        match rx.try_recv().unwrap() {
            Command::Send {
                phone,
                target,
                text,
            } => {
                assert_eq!(phone, "+79991234567");
                assert_eq!(target, "+79991234567");
                assert_eq!(text, "Тест");
            }
            _ => panic!("Expected exactly one send"),
        };
        let (tx, events) = std::sync::mpsc::channel();
        h.app.events = Some(events);
        tx.send(Event::Sent("+79991234567".into())).unwrap();
        h.frame(vec![]);
        assert!(h.app.draft.is_empty());
        assert!(!h.app.sending);
        h.app.preview = true;
    }
    #[test]
    fn email_recipients_work_for_replies_and_new_conversations() {
        let email = "parent+family@example.com";
        for new in [false, true] {
            let mut h = Harness::new(vec2(830., 570.));
            let (commands, rx) = std::sync::mpsc::channel();
            h.app.preview = false;
            h.app.commands = Some(commands);
            if new {
                h.app.new_conversation();
                h.app.recipient = format!("  {email}  ");
            } else {
                h.app.select(email);
            }
            h.app.draft = "Привет!".into();
            h.app.send();
            assert!(
                matches!(rx.try_recv(), Ok(Command::Send { phone, target, text })
                if phone == email && target == email && text == "Привет!")
            );
            assert_eq!(h.app.selected.as_deref(), Some(email));
            assert!(h.app.sending);
        }
    }
    #[test]
    fn pasting_an_image_reports_the_actual_transport_limit_without_sending() {
        let mut h = Harness::new(vec2(830., 570.));
        h.ctx.memory_mut(|m| m.request_focus(Id::unique("draft")));
        h.app.draft = "Не потерять этот текст".into();
        h.frame(vec![Input::PasteImage(Arc::new(ColorImage::new(
            [2, 2],
            vec![Color32::RED; 4],
        )))]);
        assert_eq!(h.app.error.as_deref(), Some(crate::media::UNAVAILABLE));
        assert_eq!(h.app.draft, "Не потерять этот текст");
        assert!(!h.app.sending);
    }
    #[test]
    fn keyboard_search_and_shift_enter_keep_the_expected_focus_and_text() {
        let mut h = Harness::new(vec2(830., 570.));
        h.ctx.memory_mut(|m| m.request_focus(Id::unique("draft")));
        h.app.draft = "Первая строка".into();
        h.frame(vec![]);
        h.key(Key::Enter, Modifiers::SHIFT);
        assert!(h.app.draft.contains('\n'));
        h.key(Key::F, Modifiers::CTRL);
        assert!(h.ctx.memory(|m| m.has_focus(Id::unique("search"))));
        h.frame(vec![Input::Paste("Мария".into())]);
        assert_eq!(h.app.search, "Мария");
        assert!(h.labels.contains_key("Мария"));
        assert!(!h.labels.contains_key("Александр"));
        h.key(Key::N, Modifiers::CTRL);
        assert!(h.app.new_mode);
    }
    #[test]
    fn contact_name_can_be_saved_from_the_contact_popup_and_emoji_popup_closes_outside() {
        let mut h = Harness::new(vec2(1120., 760.));
        h.click("Сведения о собеседнике");
        h.app.contact_name = "Полина ❤️".into();
        h.frame(vec![]);
        h.click("Сохранить имя");
        assert_eq!(h.app.name(), "Полина ❤️");
        h.click("Эмодзи");
        h.click("Александр");
        assert!(!h.labels.contains_key("😀"));
        assert_eq!(h.app.name(), "Александр");
    }
    #[test]
    fn message_animation_scopes_do_not_change_the_rendered_group_gaps() {
        for size in [vec2(830., 570.), vec2(1120., 760.)] {
            let h = Harness::new(size);
            assert_eq!(h.bubbles.len(), 5);
            for pair in h.bubbles.windows(2) {
                let gap = pair[1].top() - pair[0].bottom();
                assert!(
                    (gap - layout::GROUP_GAP).abs() <= 1.,
                    "Rendered group gap {gap}"
                );
            }
        }
    }
    #[test]
    fn send_keeps_the_editor_ready_for_a_second_draft_and_ack_does_not_erase_it() {
        let mut h = Harness::new(vec2(1120., 760.));
        let (commands, rx) = std::sync::mpsc::channel();
        let (events, ui_events) = std::sync::mpsc::channel();
        h.app.preview = false;
        h.app.commands = Some(commands);
        h.app.events = Some(ui_events);
        h.app.draft = "Первое сообщение".into();
        h.frame(vec![]);
        h.click("Отправить · Enter (Shift+Enter — новая строка)");
        let phone = match rx.try_recv().unwrap() {
            Command::Send { phone, text, .. } => {
                assert_eq!(text, "Первое сообщение");
                phone
            }
            _ => panic!("Expected send"),
        };
        let ready =
            h.ctx.memory(|memory| memory.has_focus(Id::unique("draft"))) && h.app.draft.is_empty();
        h.frame(vec![Input::Text("Второе сообщение".into())]);
        events.send(Event::Sent(phone)).unwrap();
        h.frame(vec![]);
        let next = h.app.draft.clone();
        h.app.preview = true;
        assert!(
            ready,
            "The composer must remain focused and clear the submitted text immediately"
        );
        assert_eq!(
            next, "Второе сообщение",
            "Acknowledgement must preserve the new draft"
        );
    }
    #[test]
    fn returning_to_the_application_focuses_the_composer() {
        let mut h = Harness::new(vec2(830., 570.));
        h.ctx
            .memory_mut(|memory| memory.surrender_focus(Id::unique("draft")));
        h.frame(vec![Input::WindowFocused(false)]);
        h.frame(vec![Input::WindowFocused(true)]);
        assert!(h.ctx.memory(|memory| memory.has_focus(Id::unique("draft"))));
        h.frame(vec![Input::Text("Сразу после Alt+Tab".into())]);
        assert_eq!(h.app.draft, "Сразу после Alt+Tab");
    }
    #[test]
    fn failed_acknowledgement_preserves_the_next_draft_and_never_retries() {
        let mut h = Harness::new(vec2(830., 570.));
        let (commands, rx) = std::sync::mpsc::channel();
        let (events, ui_events) = std::sync::mpsc::channel();
        h.app.preview = false;
        h.app.commands = Some(commands);
        h.app.events = Some(ui_events);
        h.app.draft = "Первое".into();
        h.app.send();
        assert!(matches!(rx.try_recv(), Ok(Command::Send { .. })));
        h.frame(vec![]);
        h.frame(vec![Input::Text("Следующее".into())]);
        events
            .send(Event::SendFailed("Проверь на телефоне".into()))
            .unwrap();
        h.frame(vec![]);
        h.app.preview = true;
        assert_eq!(h.app.draft, "Следующее");
        assert!(!h.app.sending);
        assert!(h.app.error.is_some());
        assert!(rx.try_recv().is_err());
    }
    #[test]
    fn settings_apply_cancel_and_activation_obey_the_focus_preference() {
        for size in [vec2(830., 570.), vec2(1120., 760.)] {
            let mut h = Harness::new(size);
            h.key(Key::Comma, Modifiers::CTRL);
            assert!(h.app.settings_open);
            assert!(h.labels.contains_key("Сохранить"));
            let button = h.labels["Сохранить"];
            assert!(
                Rect::from_min_size(Pos2::ZERO, size).contains_rect(button),
                "Settings actions must fit at minimum size"
            );
            h.click("Сразу печатать при возврате в приложение");
            h.click("Плавные анимации");
            h.click("Сохранить");
            assert!(!h.app.preferences.auto_focus);
            assert!(!h.app.motion.enabled);
            assert!(!h.app.shared_preferences.lock().unwrap().auto_focus);
            h.key(Key::Escape, Modifiers::NONE);
            assert!(!h.app.settings_open);
            h.ctx.memory_mut(|m| m.surrender_focus(Id::unique("draft")));
            h.frame(vec![Input::WindowFocused(true)]);
            assert!(!h.ctx.memory(|m| m.has_focus(Id::unique("draft"))));
            h.key(Key::Comma, Modifiers::CTRL);
            h.click("Сразу печатать при возврате в приложение");
            h.key(Key::Escape, Modifiers::NONE);
            assert!(
                !h.app.preferences.auto_focus,
                "Escape must discard unapplied changes"
            );
        }
    }
    #[test]
    fn maker_credit_opens_only_the_requested_site() {
        for size in [vec2(830., 570.), vec2(1120., 760.)] {
            let mut h = Harness::new(size);
            h.key(Key::Comma, Modifiers::CTRL);
            let bounds = h.labels["MAARK"];
            assert!(Rect::from_min_size(Pos2::ZERO, size).contains_rect(bounds));
            h.click("MAARK");
            assert_eq!(h.opened_urls, vec!["https://maark.ru"]);
        }
    }
    #[test]
    fn initial_snapshot_preserves_storage_errors_and_does_not_focus_behind_settings() {
        let mut h = Harness::new(vec2(830., 570.));
        let (events, receiver) = std::sync::mpsc::channel();
        h.app.events = Some(receiver);
        let messages = h.app.messages.clone();
        h.app.selected = None;
        h.app.draft.clear();
        h.app.open_settings();
        events
            .send(Event::StorageError("Архив сохранён, но не открыт".into()))
            .unwrap();
        events.send(Event::Snapshot(messages)).unwrap();
        h.frame(vec![]);
        assert_eq!(h.app.error.as_deref(), Some("Архив сохранён, но не открыт"));
        assert!(!h.app.focus_draft);
    }
    fn photo_fixture(color: image::Rgba<u8>) -> Vec<u8> {
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(16, 16, color))
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        crate::avatars::prepare(bytes.get_ref()).unwrap()
    }
    #[test]
    fn photo_changes_follow_the_chosen_contact_and_survive_worker_updates() {
        for size in [vec2(830., 570.), vec2(1120., 760.)] {
            let mut h = Harness::new(size);
            let phone = h.app.selected.clone().unwrap();
            let other = h
                .app
                .conversations
                .iter()
                .find(|c| c.phone != phone)
                .unwrap()
                .phone
                .clone();
            let first = photo_fixture(image::Rgba([20, 180, 80, 255]));
            assert!(h.app.set_photo(&phone, Some(first.clone())));
            h.frame(vec![]);
            let texture = h.app.photo_textures[&phone].id();
            h.click("Сведения о собеседнике");
            assert!(h.labels.contains_key("Изменить фото"));
            assert!(h.labels.contains_key("Убрать фото"));
            let first_rect = h.labels["Изменить фото"];
            let remove_rect = h.labels["Убрать фото"];
            assert!(!first_rect.intersects(remove_rect));
            h.key(Key::Escape, Modifiers::NONE);
            h.app.select(&other);
            let replacement = photo_fixture(image::Rgba([180, 30, 90, 255]));
            let (tx, rx) = std::sync::mpsc::channel();
            h.app.photo_events = Some(rx);
            h.app.photo_loading = true;
            tx.send(PhotoResult {
                phone: phone.clone(),
                result: Ok(Some(replacement.clone())),
            })
            .unwrap();
            h.frame(vec![]);
            assert_eq!(h.app.contacts.photos[&phone], replacement);
            assert!(!h.app.contacts.photos.contains_key(&other));
            assert_ne!(h.app.photo_textures[&phone].id(), texture);
            let (tx, rx) = std::sync::mpsc::channel();
            h.app.events = Some(rx);
            tx.send(Event::Contacts(Book::default())).unwrap();
            h.frame(vec![]);
            assert_eq!(h.app.contacts.photos[&phone], replacement);
            h.app.select(&phone);
            h.frame(vec![]);
            h.click("Сведения о собеседнике");
            h.click("Убрать фото");
            assert!(!h.app.contacts.photos.contains_key(&phone));
            assert!(h.labels.contains_key("Добавить фото"));
        }
    }
    #[test]
    fn cancelled_or_bad_photo_keeps_contact_and_draft_intact() {
        let mut h = Harness::new(vec2(830., 570.));
        let phone = h.app.selected.clone().unwrap();
        let photo = photo_fixture(image::Rgba([30, 80, 200, 255]));
        h.app.set_photo(&phone, Some(photo.clone()));
        h.app.draft = "Черновик при выборе фото".into();
        for result in [Ok(None), Err("Фото не открылось".into())] {
            let (tx, rx) = std::sync::mpsc::channel();
            h.app.photo_events = Some(rx);
            h.app.photo_loading = true;
            tx.send(PhotoResult {
                phone: phone.clone(),
                result,
            })
            .unwrap();
            h.frame(vec![]);
            assert_eq!(h.app.contacts.photos[&phone], photo);
            assert_eq!(h.app.draft, "Черновик при выборе фото");
            assert!(!h.app.photo_loading);
        }
        assert_eq!(h.app.error.as_deref(), Some("Фото не открылось"));
    }
    #[test]
    fn typography_is_consistent_across_every_builtin_style() {
        let h = Harness::new(vec2(830., 570.));
        let style = h.ctx.style_of(Theme::Light);
        for text_style in [
            TextStyle::Heading,
            TextStyle::Body,
            TextStyle::Button,
            TextStyle::Small,
            TextStyle::Monospace,
        ] {
            let family = &style.text_styles[&text_style].family;
            assert!(
                *family == FontFamily::Proportional
                    || *family == FontFamily::Name("Semibold".into())
            );
        }
        assert!(
            model::app_root()
                .join("assets/fonts/SF-Pro-Text-Regular.otf")
                .exists()
        );
        assert!(
            model::app_root()
                .join("assets/fonts/SF-Pro-Text-Semibold.otf")
                .exists()
        );
    }
    #[test]
    fn worst_case_content_keeps_bubbles_and_composer_inside_the_window() {
        for size in [vec2(830., 570.), vec2(1120., 760.), vec2(1600., 1000.)] {
            let mut h = Harness::new(size);
            let phone = h.app.selected.clone().unwrap();
            h.app
                .contacts
                .imported
                .insert(phone.clone(), "ОченьДлинноеИмяБезПробелов".repeat(15));
            let mut message = h.app.messages[0].clone();
            message.phone = phone;
            message.text = format!(
                "{}\n{}\n{}",
                "W".repeat(500),
                "Строка с переносами\n".repeat(30),
                "👨‍👩‍👧‍👦❤️".repeat(20)
            );
            h.app.messages = vec![message];
            h.app.rebuild_conversations();
            h.app.draft = "Многострочный черновик 👋\n".repeat(40);
            h.app.error = Some("Ошибка соединения с длинным описанием. ".repeat(20));
            for _ in 0..3 {
                h.frame(vec![]);
            }
            let regions = layout::Regions::new(Rect::from_min_size(Pos2::ZERO, size), 128., true);
            assert!(!h.bubbles.is_empty());
            for bubble in &h.bubbles {
                assert!(bubble.left() >= regions.thread.left() - 1.);
                assert!(bubble.right() <= regions.thread.right() + 1.);
            }
            let draft = h
                .labels
                .get(&h.app.draft)
                .copied()
                .or_else(|| h.labels.get("Сообщение").copied());
            if let Some(draft) = draft {
                assert!(draft.bottom() <= size.y);
                assert!(draft.left() >= regions.main.left());
            }
            let clip = h.draft_clip.expect("The draft must be rendered");
            assert!(clip.bottom() <= size.y);
            assert!(clip.top() >= regions.composer.top());
            h.key(Key::Comma, Modifiers::CTRL);
            h.app.settings_status = "Ошибка с подробным описанием. ".repeat(30);
            h.frame(vec![]);
            assert!(Rect::from_min_size(Pos2::ZERO, size).contains_rect(h.labels["Сохранить"]));
            assert!(Rect::from_min_size(Pos2::ZERO, size).contains_rect(h.labels["MAARK"]));
        }
    }
    #[test]
    fn short_contact_titles_fit_without_ellipsis() {
        for size in [vec2(830., 570.), vec2(1120., 760.)] {
            let h = Harness::new(size);
            assert!(
                !h.elided_texts.contains(&h.app.name()),
                "A short contact name must fit beside the header chevron"
            );
        }
    }
    #[test]
    fn opening_a_chat_clears_its_unread_marker_and_old_snapshots_do_not_restore_it() {
        let mut h = Harness::new(vec2(830., 570.));
        let phone = h.app.conversations[1].phone.clone();
        for message in &mut h.app.messages {
            if message.phone == phone {
                message.read = false;
            }
        }
        h.app.rebuild_conversations();
        let old_snapshot = h.app.messages.clone();
        assert!(
            h.app
                .conversations
                .iter()
                .find(|c| c.phone == phone)
                .unwrap()
                .unread
        );
        h.app.select(&phone);
        h.frame(vec![]);
        assert!(
            !h.app
                .conversations
                .iter()
                .find(|c| c.phone == phone)
                .unwrap()
                .unread,
            "Opening the chat must clear its unread dot"
        );
        let (tx, rx) = std::sync::mpsc::channel();
        h.app.events = Some(rx);
        h.app.select(&h.app.conversations[0].phone.clone());
        tx.send(Event::Snapshot(old_snapshot)).unwrap();
        h.frame(vec![]);
        assert!(
            !h.app
                .conversations
                .iter()
                .find(|c| c.phone == phone)
                .unwrap()
                .unread,
            "Polling must not restore an already-read dot"
        );
    }
    #[test]
    fn disconnected_sidebar_offers_an_immediate_connection_attempt() {
        let mut h = Harness::new(vec2(830., 570.));
        let (tx, rx) = std::sync::mpsc::channel();
        h.app.preview = false;
        h.app.commands = Some(tx);
        h.frame(vec![]);
        let button = h.labels.get("Подключить").copied();
        h.app.preview = true;
        assert!(button.is_some(), "Disconnected app needs a Connect button");
        h.app.preview = false;
        h.click("Подключить");
        let request = rx.try_recv();
        h.app.preview = true;
        assert!(
            request.is_ok(),
            "Click must request a connection immediately"
        );
    }
    #[test]
    fn connect_button_tracks_failure_retry_and_success_without_duplicate_commands() {
        for size in [vec2(830., 570.), vec2(1120., 760.)] {
            let mut h = Harness::new(size);
            let (tx, rx) = std::sync::mpsc::channel();
            let (events, inbox) = std::sync::mpsc::channel();
            h.app.preview = false;
            h.app.commands = Some(tx);
            h.app.events = Some(inbox);
            h.frame(vec![]);
            let button = h.labels["Подключить"];
            assert!(Rect::from_min_size(Pos2::ZERO, size).contains_rect(button));
            h.click("Подключить");
            let first = rx.try_recv();
            let busy = h.app.connecting;
            h.app.connect();
            let duplicate = rx.try_recv();
            h.app.preview = true;
            assert!(matches!(first, Ok(Command::Connect)));
            assert!(busy);
            assert!(duplicate.is_err());
            h.app.preview = false;
            events
                .send(Event::Connection(false, "Телефон недоступен".into()))
                .unwrap();
            h.frame(vec![]);
            let retry = h.labels.contains_key("Подключить");
            h.app.preview = true;
            assert!(retry);
            h.app.preview = false;
            h.click("Подключить");
            let second = rx.try_recv();
            h.app.preview = true;
            assert!(matches!(second, Ok(Command::Connect)));
            h.app.preview = false;
            events
                .send(Event::Connection(true, "iPhone подключён".into()))
                .unwrap();
            h.frame(vec![]);
            let connected =
                h.app.connected && !h.app.connecting && !h.labels.contains_key("Подключить");
            h.app.preview = true;
            assert!(connected);
            let status = h.text_rects["iPhone подключён"];
            let settings = h.labels["Настройки · Ctrl+,"];
            assert!((status.center().y - settings.center().y).abs() < 1.);
            assert!(status.left() > settings.right());
        }
    }
    #[test]
    fn incoming_messages_are_read_only_when_their_chat_is_visible_and_focused() {
        let mut h = Harness::new(vec2(830., 570.));
        let phone = h.app.selected.clone().unwrap();
        let mut incoming = h
            .app
            .messages
            .iter()
            .find(|m| m.phone == phone && !m.outgoing)
            .unwrap()
            .clone();
        incoming.id = "new-unseen".into();
        incoming.read = false;
        let (tx, rx) = std::sync::mpsc::channel();
        h.app.events = Some(rx);
        h.frame(vec![Input::WindowFocused(false)]);
        let mut snapshot = h.app.messages.clone();
        snapshot.push(incoming);
        tx.send(Event::Snapshot(snapshot)).unwrap();
        h.frame(vec![]);
        assert!(
            h.app
                .conversations
                .iter()
                .find(|c| c.phone == phone)
                .unwrap()
                .unread,
            "Background polling must not mark a message read"
        );
        h.app.open_settings();
        h.frame(vec![Input::WindowFocused(true)]);
        assert!(
            h.app
                .conversations
                .iter()
                .find(|c| c.phone == phone)
                .unwrap()
                .unread,
            "Settings hide the conversation"
        );
        h.key(Key::Escape, Modifiers::NONE);
        h.frame(vec![]);
        assert!(
            !h.app
                .conversations
                .iter()
                .find(|c| c.phone == phone)
                .unwrap()
                .unread
        );
    }
    #[test]
    fn search_opens_the_exact_old_message_without_losing_the_next_draft() {
        for size in [vec2(830., 570.), vec2(1120., 760.)] {
            let mut h = Harness::new(size);
            h.app.motion.enabled = true;
            let mut fixture = h.app.messages[0].clone();
            fixture.phone = h.app.selected.clone().unwrap();
            fixture.outgoing = false;
            fixture.read = true;
            h.app.messages = (0..80)
                .map(|i| {
                    let mut m = fixture.clone();
                    m.id = format!("search-{i}");
                    m.timestamp = 100 + i;
                    m.text = if i == 1 {
                        "Найди старое сообщение".into()
                    } else {
                        format!("Другое сообщение {i}")
                    };
                    m
                })
                .collect();
            h.app.rebuild_conversations();
            h.app.draft = "Следующий черновик ❤️".into();
            h.app.search = "найди старое".into();
            h.frame(vec![]);
            h.click("Открыть найденное сообщение search-1");
            for _ in 0..45 {
                h.frame(vec![]);
            }
            assert_eq!(h.app.highlighted.as_deref(), Some("search-1"));
            assert_eq!(h.app.draft, "Следующий черновик ❤️");
            let target = h
                .highlighted_bubble
                .expect("The target bubble must be highlighted");
            let regions = layout::Regions::new(Rect::from_min_size(Pos2::ZERO, size), 40., false);
            assert!(
                regions.thread.contains(target.center()),
                "Search must scroll to the actual old bubble: {target:?}"
            );
            h.click("Очистить поиск");
            assert!(h.app.search.is_empty());
            assert!(h.app.highlighted.is_none());
        }
    }
    #[test]
    fn pinning_sorts_old_chat_first_and_settings_do_not_erase_chat_preferences() {
        let mut h = Harness::new(vec2(830., 570.));
        let oldest = h.app.conversations.last().unwrap().phone.clone();
        h.app.open_settings();
        h.app.change_chat(&oldest, true, true);
        assert_eq!(h.app.conversations[0].phone, oldest);
        assert!(h.app.shared_preferences.lock().unwrap().chats[&oldest].muted);
        assert!(h.app.apply_settings(&h.ctx));
        assert!(h.app.preferences.chats[&oldest].pinned);
        h.app.change_chat(&oldest, false, false);
        assert!(!h.app.preferences.chats.contains_key(&oldest));
        for message in &mut h.app.messages {
            message.read = true;
        }
        h.app.rebuild_conversations();
        h.app.settings_open = false;
        h.app.unread_only = true;
        h.frame(vec![]);
        assert!(h.labels.contains_key("Нет непрочитанных сообщений"));
    }
    #[test]
    fn uncertain_send_requires_confirmation_and_retry_never_erases_a_new_draft() {
        let mut h = Harness::new(vec2(830., 570.));
        let mut message = h.app.messages[0].clone();
        message.phone = h.app.selected.clone().unwrap();
        message.state = SendState::Unconfirmed;
        message.outgoing = true;
        message.read = true;
        message.text = "Первое сообщение".into();
        h.app.messages.push(message.clone());
        let (tx, rx) = std::sync::mpsc::channel();
        h.app.commands = Some(tx);
        h.app.preview = false;
        h.app.draft = "Новый текст".into();
        h.app.retry = Some(message.clone());
        h.frame(vec![]);
        h.frame(vec![]);
        assert!(rx.try_recv().is_err(), "Opening confirmation must not send");
        h.click("Отмена");
        assert!(rx.try_recv().is_err());
        assert_eq!(h.app.draft, "Новый текст");
        h.app.retry = Some(message);
        h.frame(vec![]);
        h.click("Отправить ещё раз");
        let command = rx.try_recv();
        h.app.preview = true;
        assert!(matches!(command,Ok(Command::Send{text,..}) if text=="Первое сообщение"));
        assert!(rx.try_recv().is_err(), "No duplicate command");
        assert_eq!(h.app.draft, "Новый текст");
    }
    #[test]
    fn failed_state_remains_visible_after_newer_messages_arrive() {
        let mut h = Harness::new(vec2(830., 570.));
        let mut failed = h.app.messages[0].clone();
        failed.id = "failed-earlier".into();
        failed.phone = h.app.selected.clone().unwrap();
        failed.outgoing = true;
        failed.read = true;
        failed.state = SendState::Failed;
        failed.text = "Не отправлено ранее".into();
        let mut next = failed.clone();
        next.id = "accepted-later".into();
        next.state = SendState::Accepted;
        next.text = "Следующее".into();
        next.timestamp += 1;
        h.app.messages = vec![failed, next];
        h.frame(vec![]);
        assert!(h.labels.contains_key("Не отправлено"));
        assert!(h.labels.contains_key("Повторить…"));
        assert!(h.labels.contains_key("Передано iPhone"));
    }
    #[test]
    fn close_is_cancelled_in_background_logic_while_a_send_is_pending() {
        let ctx = Context::default();
        let mut app = MessagesApp::from_context(&ctx, true);
        app.sending = true;
        let mut input = RawInput::default();
        input
            .viewports
            .get_mut(&ViewportId::ROOT)
            .unwrap()
            .events
            .push(ViewportEvent::Close);
        let output = ctx.run_logic(&input, |ctx| app.receive(ctx));
        assert!(
            output.viewport_commands[&ViewportId::ROOT].contains(&ViewportCommand::CancelClose)
        );
        app.exit_requested = true;
        app.sending = false;
        let output = ctx.run_logic(&RawInput::default(), |ctx| app.receive(ctx));
        assert!(output.viewport_commands[&ViewportId::ROOT].contains(&ViewportCommand::Close));
    }
    #[test]
    fn new_settings_and_search_controls_fit_at_fractional_display_scales() {
        for scale in [1., 1.25, 1.5, 2.] {
            for size in [vec2(830., 570.), vec2(1120., 760.)] {
                let mut h = Harness::new(size);
                h.ctx.set_pixels_per_point(scale);
                h.app.search = "Привет".into();
                h.frame(vec![]);
                assert!(
                    Rect::from_min_size(Pos2::ZERO, size).contains_rect(h.labels["Очистить поиск"])
                );
                h.app.open_settings();
                h.frame(vec![]);
                h.frame(vec![]);
                let bounds =
                    layout::Regions::new(Rect::from_min_size(Pos2::ZERO, size), 40., false).main;
                for label in [
                    "Скрывать в трей при закрытии окна",
                    "Запускать с Windows в трее",
                    "Показывать текст сообщения в уведомлениях",
                ] {
                    let rect = h.labels[label];
                    assert!(
                        rect.left() >= bounds.left() + 20. && rect.right() <= bounds.right() - 20.,
                        "{label} extends past settings inset at {scale}: {rect:?}"
                    );
                }
                assert!(Rect::from_min_size(Pos2::ZERO, size).contains_rect(h.labels["Сохранить"]));
                assert!(Rect::from_min_size(Pos2::ZERO, size).contains_rect(h.labels["MAARK"]));
            }
        }
    }
    #[test]
    fn maker_credit_has_a_single_small_gap_at_both_display_scales() {
        for scale in [1., 1.25, 1.5, 2.] {
            let mut h = Harness::new(vec2(830., 570.));
            h.ctx.set_pixels_per_point(scale);
            h.key(Key::Comma, Modifiers::CTRL);
            h.frame(vec![]);
            let by = h.text_rects["Сделано by"];
            let maker = h.text_rects["MAARK"];
            let gap = maker.left() - by.right();
            assert!(
                (3.0..=5.0).contains(&gap),
                "Credit gap must be 4 DIP, got {gap} at scale {scale}"
            );
        }
    }
}
