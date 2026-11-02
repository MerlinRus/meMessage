#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod capabilities;
mod composer;
mod contacts;
mod emoji;
mod layout;
mod map;
mod media;
mod model;
mod motion;
mod notifications;
mod platform;
mod settings;
mod transport;
mod ui;
mod worker;

fn main() -> eframe::Result {
    let _ = notifications::set_app_identity();
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--probe-capabilities") {
        let result = capabilities::probe();
        let report = serde_json::json!({"ok":result.is_ok(),"result":result.as_ref().ok(),"error":result.as_ref().err()});
        let _ = std::fs::write(
            model::diagnostics_dir().join("capabilities-probe.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        );
        std::process::exit(if result.is_ok() { 0 } else { 1 });
    }
    if args.iter().any(|a| a == "--probe-history") {
        let result = worker::history_probe();
        let report = serde_json::json!({"ok":result.is_ok(),"result":result.as_ref().ok(),"error":result.as_ref().err().map(ToString::to_string)});
        let path = model::diagnostics_dir().join("history-probe.json");
        let _ = std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap());
        std::process::exit(if result.is_ok() { 0 } else { 1 });
    }
    if args.iter().any(|a| a == "--repair-shortcuts") {
        std::process::exit(if notifications::Notifications::new().is_ok() {
            0
        } else {
            1
        });
    }
    if args.iter().any(|a| a == "--probe-contacts") {
        let result = transport::find_iphone().and_then(contacts::probe);
        let report = serde_json::json!({"ok":result.is_ok(),"result":result.as_ref().ok(),"error":result.as_ref().err().map(ToString::to_string)});
        let path = model::diagnostics_dir().join("contacts-probe.json");
        let _ = std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap());
        std::process::exit(if result.is_ok() { 0 } else { 1 });
    }
    if args.iter().any(|a| a == "--probe-attachments") {
        let result = worker::attachment_probe();
        let report = serde_json::json!({"ok":result.is_ok(),"result":result.as_ref().ok(),"error":result.as_ref().err().map(ToString::to_string)});
        let path = model::diagnostics_dir().join("attachment-probe.json");
        let _ = std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap());
        std::process::exit(if result.is_ok() { 0 } else { 1 });
    }
    if args.iter().any(|a| a == "--notify-test") {
        let activated = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let clicked = activated.clone();
        let mut notifications = notifications::Notifications::new();
        let result = notifications
            .as_mut()
            .map_err(|error| error.clone())
            .and_then(|n| {
                n.show(
                    "iPhone Bridge",
                    "Проверка уведомлений: сообщения с iPhone будут появляться здесь.",
                    Some(std::sync::Arc::new(move || {
                        clicked.store(true, std::sync::atomic::Ordering::Relaxed);
                    })),
                )
            });
        let report = serde_json::json!({"ok":result.is_ok(),"error":result.as_ref().err()});
        let path = model::diagnostics_dir().join("notification-test.json");
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&path, serde_json::to_vec_pretty(&report).unwrap());
        if result.is_ok() {
            for _ in 0..300 {
                if activated.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            let history = notifications.as_ref().ok().map(|n| n.history_count());
            let report = serde_json::json!({"ok":true,"activated":activated.load(std::sync::atomic::Ordering::Relaxed),"history":history,"error":null});
            let _ = std::fs::write(&path, serde_json::to_vec_pretty(&report).unwrap());
        }
        std::process::exit(if result.is_ok() { 0 } else { 1 });
    }
    if args.iter().any(|a| a == "--probe") {
        let mut messages = Vec::new();
        let result = worker::refresh(&mut messages);
        let report = serde_json::json!({"ok":result.is_ok(),"messages":messages.len(),"text_received":messages.iter().any(|m|!m.text.is_empty()),"error":result.as_ref().err().map(ToString::to_string)});
        let report_path = model::diagnostics_dir().join("probe.json");
        if let Some(parent) = report_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(report_path, serde_json::to_vec_pretty(&report).unwrap());
        std::process::exit(if result.is_ok() { 0 } else { 1 });
    }
    let preview = args.iter().any(|a| a == "--preview");
    let compact = preview && args.iter().any(|a| a == "--compact");
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size(if compact { [830., 570.] } else { [1120., 760.] })
            .with_min_inner_size([830., 570.])
            .with_decorations(false)
            .with_transparent(false)
            .with_icon(ui::app_icon()),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        if preview {
            "Предпросмотр · iPhone Bridge"
        } else {
            "Сообщения · iPhone Bridge"
        },
        options,
        Box::new(move |cc| Ok(Box::new(ui::MessagesApp::new(cc, preview)))),
    )
}
