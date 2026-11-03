use eframe::egui::*;

#[derive(Clone, Copy)]
pub enum Icon {
    Compose,
    Filter,
    Video,
    Plus,
    Smile,
    Arrow,
    Chevron,
    Search,
    Settings,
    Person,
    Message,
    Photo,
    Close,
    Minimize,
    Maximize,
}
impl Icon {
    pub fn source(self) -> (&'static str, &'static [u8]) {
        macro_rules! svg {
            ($name:literal) => {
                (
                    concat!("bytes://icons/", $name, ".svg"),
                    include_bytes!(concat!("../assets/icons/", $name, ".svg")),
                )
            };
        }
        match self {
            Self::Compose => svg!("compose"),
            Self::Filter => svg!("filter"),
            Self::Video => svg!("video"),
            Self::Plus => svg!("plus"),
            Self::Smile => svg!("smile"),
            Self::Arrow => svg!("arrow"),
            Self::Chevron => svg!("chevron"),
            Self::Search => svg!("search"),
            Self::Settings => svg!("settings"),
            Self::Person => svg!("person"),
            Self::Message => svg!("message"),
            Self::Photo => svg!("photo"),
            Self::Close => svg!("close"),
            Self::Minimize => svg!("minimize"),
            Self::Maximize => svg!("maximize"),
        }
    }
}
pub fn paint(ui: &Ui, rect: Rect, icon: Icon, color: Color32) {
    // Snap one common side in physical pixels, so fractional Windows DPI
    // cannot round the width and height to different pixel counts.
    let pixels = ui.pixels_per_point();
    let side = (rect.width().min(rect.height()) * pixels).round().max(1.);
    let origin = (rect.center() * pixels - Vec2::splat(side * 0.5)).round();
    let rect = Rect::from_min_size(origin / pixels, Vec2::splat(side / pixels));
    let (uri, bytes) = icon.source();
    Image::from_bytes(uri, bytes).tint(color).paint_at(ui, rect);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_interface_icons_are_valid_renderable_svg() {
        for icon in [
            Icon::Compose,
            Icon::Filter,
            Icon::Video,
            Icon::Plus,
            Icon::Smile,
            Icon::Arrow,
            Icon::Chevron,
            Icon::Search,
            Icon::Settings,
            Icon::Person,
            Icon::Message,
            Icon::Photo,
            Icon::Close,
            Icon::Minimize,
            Icon::Maximize,
        ] {
            let (_, svg) = icon.source();
            let image = egui_extras::image::load_svg_bytes(svg, &Default::default()).unwrap();
            assert_eq!(image.size, [24, 24]);
            assert!(image.pixels.iter().any(|p| p.a() > 0));
        }
    }
    #[test]
    fn avatar_artwork_stays_inside_its_circle() {
        let image =
            egui_extras::image::load_svg_bytes(Icon::Person.source().1, &Default::default())
                .unwrap();
        for y in 0..image.size[1] {
            for x in 0..image.size[0] {
                if image.pixels[y * image.size[0] + x].a() > 128 {
                    assert!(
                        ((x as f32 + 0.5 - 12.).hypot(y as f32 + 0.5 - 12.)) <= 12.5,
                        "Avatar pixels outside circle: {x},{y}"
                    );
                }
            }
        }
    }
    #[test]
    fn settings_svg_keeps_square_proportions_in_a_wide_button() {
        for scale in [1., 1.25, 1.5, 2.] {
            let ctx = Context::default();
            ctx.set_pixels_per_point(scale);
            egui_extras::install_image_loaders(&ctx);
            let mut seen = None;
            for _ in 0..2 {
                let mut output = ctx.run_ui(Default::default(), |ui| {
                    paint(
                        ui,
                        Rect::from_min_size(pos2(10., 10.), vec2(15., 11.)),
                        Icon::Settings,
                        Color32::WHITE,
                    )
                });
                output.textures_delta.clear();
                for shape in output.shapes {
                    match shape.shape {
                        epaint::Shape::Mesh(mesh) => seen = Some(mesh.calc_bounds()),
                        epaint::Shape::Rect(rect) if rect.brush.is_some() => seen = Some(rect.rect),
                        _ => {}
                    }
                }
            }
            let rect = seen.expect("Settings SVG must render");
            assert!(
                (rect.width() - rect.height()).abs() < 0.1,
                "Stretched settings SVG: {rect:?}"
            );
        }
    }
}
