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
}
