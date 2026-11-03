use eframe::egui::{text_edit::TextEditOutput, *};

pub fn edit(ui: &mut Ui, draft: &mut String, input: Rect, enabled: bool) -> TextEditOutput {
    let mut child = ui.new_child(UiBuilder::new().max_rect(Rect::from_min_max(
        input.min + vec2(13., 0.),
        input.max - vec2(43., 0.),
    )));
    let output = child
        .add_enabled_ui(enabled, |ui| {
            ScrollArea::vertical()
                .id_salt("draft_scroll")
                .max_height(input.height())
                .auto_shrink([false, false])
                .scroll_bar_visibility(scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(ui, |ui| {
                    TextEdit::multiline(draft)
                        .id(Id::unique("draft"))
                        .frame(Frame::NONE)
                        .margin(Margin::ZERO)
                        .min_size(vec2(0., input.height()))
                        .align(Align2::LEFT_CENTER)
                        .font(FontId::proportional(14.))
                        .hint_text("Сообщение")
                        .return_key(Some(KeyboardShortcut::new(Modifiers::SHIFT, Key::Enter)))
                        .desired_width(f32::INFINITY)
                        .desired_rows(1)
                        .show(ui)
                })
                .inner
        })
        .inner;
    // The scroll content can be taller than the composer. Expose the visible
    // editor bounds to accessibility clients, rather than off-screen text rows.
    let visible = output.response.rect.intersect(input);
    ui.ctx().accesskit_node_builder(output.response.id, |node| {
        node.set_bounds(accesskit::Rect {
            x0: visible.left() as f64,
            y0: visible.top() as f64,
            x1: visible.right() as f64,
            y1: visible.bottom() as f64,
        })
    });
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn placeholder_and_typed_text_share_vertical_center() {
        let ctx = Context::default();
        let mut fonts = FontDefinitions::default();
        let bytes = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("assets/fonts/SF-Pro-Text-Regular.otf"),
        )
        .unwrap();
        fonts.font_data.insert(
            "SF".into(),
            std::sync::Arc::new(FontData::from_owned(bytes)),
        );
        fonts
            .families
            .get_mut(&FontFamily::Proportional)
            .unwrap()
            .insert(0, "SF".into());
        ctx.set_fonts(fonts);
        let input = Rect::from_min_size(pos2(20., 20.), vec2(460., 44.));
        for text in ["", "Привет, как дела?"] {
            let mut draft = text.to_owned();
            let mut center = 0.;
            let mut frame = ctx.run_ui(Default::default(), |ui| {
                let output = edit(ui, &mut draft, input, true);
                center = output.galley_pos.y + output.galley.size().y / 2.;
            });
            frame.textures_delta.clear();
            assert!(
                (center - input.center().y).abs() <= 1.,
                "{text:?}: text center {center}, input center {}",
                input.center().y
            );
        }
    }
}
