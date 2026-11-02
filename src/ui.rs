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

#[derive(Clone, Copy)]
enum Icon {
    Compose,
    Filter,
    Video,
    Plus,
    Smile,
    Arrow,
    Chevron,
    Search,
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
}

pub fn setup(ctx: &Context) {
    ctx.set_theme(Theme::Light);
    let mut fonts = FontDefinitions::default();
    let font_root = model::data_path()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("assets/fonts");
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
    ctx.set_style_of(Theme::Light, style);
}

impl MessagesApp {
    pub fn new(cc: &eframe::CreationContext<'_>, preview: bool) -> Self {
        Self::from_context(&cc.egui_ctx, preview)
    }
    fn from_context(ctx: &Context, preview: bool) -> Self {
        setup(ctx);
        let motion = crate::motion::Motion::new();
        ctx.style_mut_of(Theme::Light, |style| {
            style.animation_time = if motion.enabled {
                crate::motion::POPUP_TIME
            } else {
                0.
            }
        });
        let emoji_root = model::data_path()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("assets/emoji");
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
        let (commands, events) = if preview {
            (None, None)
        } else {
            let (c, e) = worker::start(ctx.clone(), model::data_path(), shared_contacts.clone());
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
        };
        if preview {
            app.seed_preview();
        }
        app
    }
    fn receive(&mut self, ctx: &Context) {
        let events: Vec<Event> = self
            .events
            .as_ref()
            .map(|r| r.try_iter().collect())
            .unwrap_or_default();
        for event in events {
            match event {
                Event::Snapshot(messages) => {
                    if self.connected {
                        self.motion.register(&self.messages, &messages);
                    }
                    let changed = self.messages.len() != messages.len();
                    self.messages = messages;
                    self.rebuild_conversations();
                    if self.selected.is_none()
                        && !self.new_mode
                        && let Some(c) = self.conversations.first()
                    {
                        self.selected = Some(c.phone.clone());
                    }
                    if changed {
                        self.scroll_to_bottom = true;
                    }
                }
                Event::Connection(connected, text) => {
                    self.connected = connected;
                    if connected {
                        self.status = text;
                        self.connection_error = None;
                    } else {
                        self.status = "Нет связи с iPhone".into();
                        self.connection_error = Some(text);
                    }
                }
                Event::Sent(phone) => {
                    self.sending = false;
                    self.drafts.remove(&phone);
                    if self.selected.as_deref() == Some(&phone) {
                        self.draft.clear();
                    }
                    self.scroll_to_bottom = true;
                }
                Event::OpenConversation(phone) => {
                    self.select(&phone);
                    ctx.send_viewport_cmd(ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(ViewportCommand::Focus);
                }
                Event::NotificationError(error) => {
                    self.notification_error = Some(error);
                }
                Event::Contacts(book) => {
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
                    self.error = Some(error);
                }
            }
        }
    }
    fn rebuild_conversations(&mut self) {
        self.conversations = model::conversations(&self.messages);
        self.contacts.apply(&mut self.conversations);
    }
    fn contact_phone(&self) -> Option<String> {
        self.selected.clone().or_else(|| {
            map::normalize_phone(&self.recipient)
                .ok()
                .map(|phone| model::key(&phone))
        })
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
                RichText::new(model::format_phone(&phone))
                    .small()
                    .color(SECONDARY),
            );
            ui.separator();
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
            if ui.button("Скопировать номер").clicked() {
                ui.ctx().copy_text(phone);
                ui.close();
            }
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
        if self.sending {
            return;
        }
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
    fn new_conversation(&mut self) {
        if self.sending {
            return;
        }
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
        let target = match map::normalize_phone(input) {
            Ok(phone) => phone,
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
                self.selected = Some(phone);
                self.new_mode = false;
                self.sending = true;
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
        ui.painter().rect_filled(rect, 18, SIDE);
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
                let stroke = Stroke::new(1.0_f32, Color32::from_black_alpha(150));
                match i {
                    0 => {
                        ui.painter()
                            .line_segment([center + vec2(-2., -2.), center + vec2(2., 2.)], stroke);
                        ui.painter()
                            .line_segment([center + vec2(2., -2.), center + vec2(-2., 2.)], stroke);
                    }
                    1 => {
                        ui.painter()
                            .line_segment([center + vec2(-3., 0.), center + vec2(3., 0.)], stroke);
                    }
                    _ => {
                        ui.painter().rect_stroke(
                            Rect::from_center_size(center, vec2(5., 5.)),
                            0,
                            stroke,
                            StrokeKind::Inside,
                        );
                    }
                }
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
            ui.painter(),
            Rect::from_center_size(pos2(search.left() + 16., search.center().y), vec2(13., 13.)),
            Icon::Search,
            SECONDARY,
        );
        let response = ui.put(
            Rect::from_min_max(
                pos2(search.left() + 31., search.top() + 5.),
                pos2(search.right() - 11., search.bottom() - 3.),
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
        let rows = Rect::from_min_max(
            pos2(rect.left() + 5., search.bottom() + 10.),
            pos2(rect.right() - 5., rect.bottom() - 30.),
        );
        let query = self.search.to_lowercase();
        let filtered: Vec<Conversation> =
            self.conversations
                .iter()
                .filter(|c| {
                    (!self.unread_only || c.unread)
                        && (query.is_empty()
                            || c.name.to_lowercase().contains(&query)
                            || c.phone.contains(&query)
                            || self.messages.iter().any(|m| {
                                m.phone == c.phone && m.text.to_lowercase().contains(&query)
                            }))
                })
                .cloned()
                .collect();
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
                        for conversation in filtered {
                            let (row, response) = ui.allocate_exact_size(
                                vec2(rows.width() - 3., layout::ROW_HEIGHT),
                                Sense::click(),
                            );
                            response.widget_info(|| {
                                WidgetInfo::labeled(
                                    Role::ListBoxOption,
                                    !self.sending,
                                    &conversation.name,
                                )
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
                            avatar(ui.painter(), pos2(row.left() + 30., row.center().y), 21.);
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
                                row.right() - 12. - left,
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
                        }
                        if self.conversations.is_empty()
                            || !self.search.is_empty() && ui.min_rect().height() < 10.
                        {
                            ui.label(
                                RichText::new(if self.search.is_empty() {
                                    "Пока нет диалогов"
                                } else {
                                    "Ничего не найдено"
                                })
                                .size(12.)
                                .color(SECONDARY),
                            );
                        }
                    });
            },
        );
        let center = pos2(rect.center().x, rect.bottom() - 13.);
        let status = if self.preview {
            "Предпросмотр · без подключения"
        } else {
            &self.status
        };
        let galley =
            ui.painter()
                .layout_no_wrap(status.into(), FontId::proportional(10.5), SECONDARY);
        let x = center.x - (galley.size().x + 12.) / 2.;
        ui.painter().circle_filled(
            pos2(x, center.y + 1.),
            2.5,
            if self.connected {
                Color32::from_rgb(52, 199, 89)
            } else {
                Color32::from_rgb(153, 153, 160)
            },
        );
        ui.painter()
            .galley(pos2(x + 8., center.y - 6.), galley, SECONDARY);
    }

    fn header(&mut self, ui: &mut Ui, rect: Rect) {
        let compose = Rect::from_min_size(pos2(rect.left() + 10., rect.top() + 8.), vec2(32., 32.));
        if icon_button(
            ui,
            compose,
            Icon::Compose,
            "Новое сообщение · Ctrl+N",
            !self.sending,
        )
        .clicked()
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
                    .hint_text("Номер телефона")
                    .font(FontId::proportional(14.)),
            );
            if self.focus_recipient {
                response.request_focus();
                self.focus_recipient = false;
            }
        } else {
            let name = self.name();
            let center = pos2(rect.center().x, rect.top() + 24.);
            avatar(ui.painter(), center, 15.5);
            let galley = ui.painter().layout_no_wrap(
                name,
                FontId::new(11., FontFamily::Name("Semibold".into())),
                TEXT,
            );
            let width = (galley.size().x + 29.).min(rect.width() - 150.);
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
            ui.painter()
                .with_clip_rect(capsule.shrink2(vec2(10., 1.)))
                .galley(pos2(capsule.left() + 10., capsule.top() + 4.), galley, TEXT);
            paint_icon(
                ui.painter(),
                Rect::from_center_size(
                    pos2(capsule.right() - 9., capsule.center().y),
                    vec2(5., 8.),
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
            p.rect_stroke(
                Rect::from_center_size(center - vec2(0., 36.), vec2(52., 39.)),
                15,
                Stroke::new(2.0_f32, Color32::from_rgb(207, 207, 214)),
                StrokeKind::Inside,
            );
            p.line_segment(
                [center + vec2(-12., -17.), center + vec2(-18., -7.)],
                Stroke::new(2.0_f32, Color32::from_rgb(207, 207, 214)),
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
                    "Укажи номер и напиши сообщение."
                } else {
                    "Выбери диалог или начни новый."
                },
                FontId::proportional(13.),
                SECONDARY,
            );
            return;
        }
        let force = self.scroll_to_bottom;
        self.scroll_to_bottom = false;
        ui.scope_builder(
            UiBuilder::new()
                .max_rect(rect)
                .layout(Layout::top_down(Align::Min)),
            |ui| {
                let area = ScrollArea::vertical()
                    .id_salt(("thread", self.selected.clone()))
                    .auto_shrink([false, false])
                    .stick_to_bottom(true);
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
                            });
                        });
                        if last {
                            ui.add_space(layout::GROUP_GAP - layout::MESSAGE_GAP);
                        }
                        if message.outgoing && index + 1 == records.len() {
                            let state = match message.state {
                                SendState::Pending => "Отправляется…",
                                SendState::Accepted => "Передано iPhone",
                                SendState::Unconfirmed => "Проверь отправку на iPhone",
                                _ => "",
                            };
                            ui.allocate_ui_with_layout(
                                vec2(width, 17.),
                                Layout::right_to_left(Align::Center),
                                |ui| {
                                    ui.label(RichText::new(state).size(10.).color(SECONDARY));
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
            !self.sending,
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
        if crate::composer::edit(ui, &mut self.draft, input, !self.sending)
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
            !self.sending,
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
        [0.; 4]
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
        if self.sending && ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.error = Some("Дождись результата отправки, затем закрой окно.".into());
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::CTRL, Key::N)) {
            self.new_conversation();
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::CTRL, Key::F)) {
            self.focus_search = true;
        }
        let enter = ctx.memory(|m| m.has_focus(Id::new("draft")))
            && !ctx.input(|i| i.modifiers.shift)
            && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));
        CentralPanel::default()
            .frame(Frame::NONE)
            .show(root_ui, |ui| {
                let bounds = ui.max_rect();
                ui.painter().rect_filled(bounds, 13, Color32::WHITE);
                ui.painter().rect_stroke(
                    bounds.shrink(0.5),
                    13,
                    Stroke::new(1.0_f32, Color32::from_rgb(206, 206, 213)),
                    StrokeKind::Inside,
                );
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
        if enter {
            self.send();
        }
        // No constant render loop: only input, worker events, and native text-caret blinking repaint.
        ctx.request_repaint_after(Duration::from_secs(12));
    }
}
impl Drop for MessagesApp {
    fn drop(&mut self) {
        if let Some(commands) = &self.commands {
            let _ = commands.send(Command::Stop);
        }
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
fn avatar(p: &Painter, center: Pos2, radius: f32) {
    p.circle_filled(center, radius, Color32::from_rgb(199, 199, 206));
    p.circle_filled(
        center + vec2(0., -radius * 0.24),
        radius * 0.28,
        Color32::from_rgb(249, 249, 251),
    );
    let body = Rect::from_center_size(
        center + vec2(0., radius * 0.48),
        vec2(radius * 1.24, radius * 0.69),
    );
    p.add(Shape::ellipse_filled(
        body.center(),
        body.size() / 2.,
        Color32::from_rgb(249, 249, 251),
    ));
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
        ui.painter(),
        rect.shrink(if matches!(icon, Icon::Arrow) { 7. } else { 7.5 }),
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
fn paint_icon(p: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let point = |x: f32, y: f32| {
        pos2(
            rect.left() + x * rect.width(),
            rect.top() + y * rect.height(),
        )
    };
    let stroke = Stroke::new(1.45_f32, color);
    let line = |a: (f32, f32), b: (f32, f32)| {
        p.line_segment([point(a.0, a.1), point(b.0, b.1)], stroke);
    };
    match icon {
        Icon::Plus => {
            line((0.05, 0.5), (0.95, 0.5));
            line((0.5, 0.05), (0.5, 0.95));
        }
        Icon::Arrow => {
            line((0.15, 0.44), (0.5, 0.08));
            line((0.5, 0.08), (0.85, 0.44));
            line((0.5, 0.08), (0.5, 0.95));
        }
        Icon::Chevron => {
            line((0.1, 0.1), (0.8, 0.5));
            line((0.8, 0.5), (0.1, 0.9));
        }
        Icon::Filter => {
            line((0.05, 0.22), (0.95, 0.22));
            line((0.2, 0.5), (0.8, 0.5));
            line((0.36, 0.78), (0.64, 0.78));
        }
        Icon::Search => {
            p.circle_stroke(point(0.4, 0.4), rect.width() * 0.34, stroke);
            line((0.66, 0.66), (0.97, 0.97));
        }
        Icon::Smile => {
            p.circle_stroke(rect.center(), rect.width() * 0.46, stroke);
            p.circle_filled(point(0.33, 0.38), 0.8, color);
            p.circle_filled(point(0.67, 0.38), 0.8, color);
            p.add(Shape::line(
                vec![
                    point(0.27, 0.61),
                    point(0.38, 0.72),
                    point(0.5, 0.75),
                    point(0.62, 0.72),
                    point(0.73, 0.61),
                ],
                stroke,
            ));
        }
        Icon::Video => {
            p.rect_stroke(
                Rect::from_min_max(point(0.03, 0.15), point(0.66, 0.85)),
                3,
                stroke,
                StrokeKind::Inside,
            );
            p.add(Shape::closed_line(
                vec![
                    point(0.66, 0.37),
                    point(0.98, 0.17),
                    point(0.98, 0.83),
                    point(0.66, 0.63),
                ],
                stroke,
            ));
        }
        Icon::Compose => {
            p.add(Shape::line(
                vec![
                    point(0.6, 0.15),
                    point(0.12, 0.15),
                    point(0.12, 0.92),
                    point(0.88, 0.92),
                    point(0.88, 0.47),
                ],
                stroke,
            ));
            p.add(Shape::closed_line(
                vec![
                    point(0.4, 0.6),
                    point(0.45, 0.39),
                    point(0.84, 0.02),
                    point(0.99, 0.16),
                    point(0.6, 0.55),
                ],
                stroke,
            ));
        }
    }
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
            };
            harness.frame(vec![]);
            harness.frame(vec![]);
            harness
        }
        fn frame(&mut self, events: Vec<Input>) {
            self.time += 1. / 60.;
            self.app.receive(&self.ctx);
            let mut output = self.ctx.run_ui(
                RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, self.size)),
                    time: Some(self.time),
                    events,
                    ..Default::default()
                },
                |ui| self.app.render(ui),
            );
            self.labels.clear();
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
}
