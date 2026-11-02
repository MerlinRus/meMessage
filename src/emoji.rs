use eframe::egui::{
    self, ColorImage,
    epaint::text::{FontPriority, GlyphBitmap, GlyphRasterizer, RasterizedGlyph},
    vec2,
};
use serde::Deserialize;
use std::{collections::HashMap, path::Path, sync::Arc};

#[derive(Deserialize)]
struct Raw {
    name: Option<String>,
    unified: String,
    non_qualified: Option<String>,
    short_name: String,
    #[serde(default)]
    short_names: Vec<String>,
    category: String,
    sheet_x: u32,
    sheet_y: u32,
    #[serde(default)]
    has_img_apple: bool,
    #[serde(default)]
    sort_order: u32,
    #[serde(default)]
    skin_variations: HashMap<String, Variant>,
}
#[derive(Deserialize)]
struct Variant {
    unified: String,
    sheet_x: u32,
    sheet_y: u32,
    #[serde(default)]
    has_img_apple: bool,
}
#[derive(Clone)]
pub struct Emoji {
    pub text: String,
    pub name: String,
    pub category: String,
    pub search: String,
    pub x: u32,
    pub y: u32,
}
pub struct Catalog {
    pub entries: Vec<Emoji>,
    lookup: HashMap<String, usize>,
    sheet: image::RgbaImage,
}
fn decode(value: &str) -> Option<String> {
    value
        .split('-')
        .map(|part| u32::from_str_radix(part, 16).ok().and_then(char::from_u32))
        .collect()
}
impl Catalog {
    pub fn load(root: &Path) -> Result<Arc<Self>, String> {
        let data = std::fs::read(root.join("emoji.json")).map_err(|e| e.to_string())?;
        let mut raw: Vec<Raw> = serde_json::from_slice(&data).map_err(|e| e.to_string())?;
        raw.sort_by_key(|r| r.sort_order);
        let sheet = image::open(root.join("apple-32.png"))
            .map_err(|e| e.to_string())?
            .to_rgba8();
        let mut entries = Vec::new();
        let mut aliases = Vec::new();
        for raw in raw {
            if !raw.has_img_apple {
                continue;
            }
            let Some(text) = decode(&raw.unified) else {
                continue;
            };
            let name = raw.name.unwrap_or_else(|| raw.short_name.clone());
            let mut search =
                format!("{} {} {}", name, raw.short_name, raw.short_names.join(" ")).to_lowercase();
            for (term, russian) in [
                ("heart", "сердце любовь"),
                ("smil", "улыбка лицо"),
                ("laugh", "смех смешно"),
                ("thumb", "палец рука"),
                ("flag", "флаг"),
                ("face", "лицо"),
                ("cat", "кот кошка"),
                ("dog", "собака"),
                ("flower", "цветок"),
                ("kiss", "поцелуй"),
                ("fire", "огонь"),
                ("party", "праздник"),
                ("cry", "плач слезы"),
                ("hand", "рука"),
                ("family", "семья"),
            ] {
                if search.contains(term) {
                    search.push(' ');
                    search.push_str(russian);
                }
            }
            let index = entries.len();
            aliases.push((text.clone(), index));
            if let Some(alias) = raw.non_qualified.and_then(|s| decode(&s)) {
                aliases.push((alias, index));
            }
            entries.push(Emoji {
                text,
                name: name.clone(),
                category: raw.category.clone(),
                search: search.clone(),
                x: raw.sheet_x,
                y: raw.sheet_y,
            });
            let mut variants: Vec<_> = raw.skin_variations.into_iter().collect();
            variants.sort_by(|a, b| a.0.cmp(&b.0));
            for (tone, variant) in variants {
                if !variant.has_img_apple {
                    continue;
                }
                let Some(text) = decode(&variant.unified) else {
                    continue;
                };
                let index = entries.len();
                aliases.push((text.clone(), index));
                entries.push(Emoji {
                    text,
                    name: format!("{name} · {tone}"),
                    category: raw.category.clone(),
                    search: format!("{search} оттенок кожи skin tone"),
                    x: variant.sheet_x,
                    y: variant.sheet_y,
                });
            }
        }
        if entries
            .iter()
            .any(|e| e.x * 34 + 33 > sheet.width() || e.y * 34 + 33 > sheet.height())
        {
            return Err("Emoji atlas coordinates are invalid".into());
        }
        let mut lookup = HashMap::new();
        for (text, index) in aliases {
            lookup.insert(text.clone(), index);
            lookup.entry(text.replace('\u{fe0f}', "")).or_insert(index);
        }
        Ok(Arc::new(Self {
            entries,
            lookup,
            sheet,
        }))
    }
    pub fn install(self: &Arc<Self>, ctx: &egui::Context) {
        let catalog = Arc::clone(self);
        ctx.add_glyph_rasterizer(
            GlyphRasterizer::new("iphone-bridge.apple-emoji-17", move |request| {
                if request.cluster.is_ascii() || request.cluster.contains('\u{fe0e}') {
                    return None;
                }
                let index = *catalog.lookup.get(request.cluster)?;
                let entry = &catalog.entries[index];
                let tile = image::imageops::crop_imm(
                    &catalog.sheet,
                    entry.x * 34 + 1,
                    entry.y * 34 + 1,
                    32,
                    32,
                )
                .to_image();
                let size = (request.font_size_px * 1.08).round().clamp(8., 128.) as u32;
                let pixels = image::imageops::resize(
                    &tile,
                    size,
                    size,
                    image::imageops::FilterType::Triangle,
                );
                Some(RasterizedGlyph {
                    bitmap: GlyphBitmap {
                        image: ColorImage::from_rgba_unmultiplied(
                            [size as usize, size as usize],
                            pixels.as_raw(),
                        ),
                        offset_px: vec2(0., -(size as f32) * 0.82),
                        is_color: true,
                    },
                    advance_px: size as f32,
                })
            })
            .with_priority(FontPriority::Highest),
        );
    }
    #[cfg(test)]
    pub fn has(&self, text: &str) -> bool {
        self.lookup.contains_key(text)
    }
}
pub fn category_name(category: &str) -> &str {
    match category {
        "Smileys & Emotion" => "Лица и эмоции",
        "People & Body" => "Люди и жесты",
        "Animals & Nature" => "Животные и природа",
        "Food & Drink" => "Еда и напитки",
        "Travel & Places" => "Места и транспорт",
        "Activities" => "Занятия",
        "Objects" => "Предметы",
        "Symbols" => "Символы",
        "Flags" => "Флаги",
        _ => category,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn full_apple_catalog_includes_joined_emoji_tones_and_flags() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/emoji");
        let catalog = Catalog::load(&root).unwrap();
        assert!(catalog.entries.len() > 3900);
        for emoji in ["😀", "❤️", "👍🏽", "👨‍👩‍👧‍👦", "🇷🇺", "1️⃣"]
        {
            assert!(catalog.has(emoji), "{emoji}");
        }
        assert!(!catalog.has("A"));
    }
    #[test]
    fn joined_emoji_render_in_color_without_changing_message_text() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/emoji");
        let catalog = Catalog::load(&root).unwrap();
        let ctx = egui::Context::default();
        catalog.install(&ctx);
        let original = "Привет 😀 ❤️ 👍🏽 👨‍👩‍👧‍👦 🇷🇺 1️⃣";
        let mut output = ctx.run_ui(Default::default(), |ui| {
            let galley = ui.painter().layout_no_wrap(
                original.into(),
                egui::FontId::proportional(14.),
                egui::Color32::WHITE,
            );
            assert_eq!(galley.job.text, original);
            assert_eq!(
                galley
                    .rows
                    .iter()
                    .flat_map(|row| &row.glyphs)
                    .filter(|glyph| glyph.is_color)
                    .count(),
                6
            );
        });
        output.textures_delta.clear();
    }
}
