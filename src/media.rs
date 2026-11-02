pub const UNAVAILABLE: &str =
    "iPhone не передаёт фотографии через этот Bluetooth-канал. Отправь фото с телефона.";
pub fn is_photo_placeholder(text: &str) -> bool {
    let text = text.trim().to_ascii_lowercase();
    if text.len() > 80 {
        return false;
    }
    text.strip_prefix("attachment:")
        .or_else(|| text.strip_prefix("attachment "))
        .is_some_and(|tail| {
            tail.split_whitespace()
                .last()
                .is_some_and(|word| ["image", "images", "photo", "photos"].contains(&word))
        })
}
pub fn display(text: &str) -> &str {
    if is_photo_placeholder(text) {
        "Фото\nИзображение доступно на iPhone.\nBluetooth не передаёт сам файл."
    } else {
        text
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn photo_placeholders_are_explained_without_inventing_image_data() {
        for text in [
            "Attachment: 1 Image",
            "Attachment One Image",
            "Attachment: 2 Images",
        ] {
            assert!(is_photo_placeholder(text));
            assert!(display(text).starts_with("Фото\n"));
        }
        assert_eq!(
            display("Посмотри на эту фотографию"),
            "Посмотри на эту фотографию"
        );
    }
}
