use image::ImageDecoder;
use std::{
    io::{self, Cursor, Read},
    path::Path,
};

pub fn from_file(path: &Path) -> io::Result<Vec<u8>> {
    let file = std::fs::File::open(path)?;
    if file.metadata()?.len() > 16 * 1024 * 1024 {
        return Err(io::Error::other("Выбери фото размером до 16 МБ."));
    }
    let mut bytes = Vec::new();
    std::io::Read::take(file, 16 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
    prepare(&bytes)
}
pub fn prepare(bytes: &[u8]) -> io::Result<Vec<u8>> {
    if bytes.len() > 16 * 1024 * 1024 {
        return Err(io::Error::other("Выбери фото размером до 16 МБ."));
    }
    let mut reader = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let mut decoder = reader
        .into_decoder()
        .map_err(|_| io::Error::other("Фото не открылось. Выбери JPEG, PNG или WebP."))?;
    if decoder.total_bytes() > 64 * 1024 * 1024 {
        return Err(io::Error::other(
            "Фото слишком большое. Выбери изображение с меньшим разрешением.",
        ));
    }
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut photo = image::DynamicImage::from_decoder(decoder)
        .map_err(|_| io::Error::other("Фото не открылось. Выбери JPEG, PNG или WebP."))?;
    photo.apply_orientation(orientation);
    let mut output = Cursor::new(Vec::new());
    photo
        .resize_to_fill(256, 256, image::imageops::FilterType::Lanczos3)
        .write_to(&mut output, image::ImageFormat::Png)
        .map_err(io::Error::other)?;
    Ok(output.into_inner())
}
pub fn texture(ctx: &eframe::egui::Context, bytes: &[u8]) -> Option<eframe::egui::TextureHandle> {
    let photo = image::load_from_memory(bytes).ok()?.to_rgba8();
    let size = [photo.width() as usize, photo.height() as usize];
    Some(ctx.load_texture(
        "contact-photo",
        eframe::egui::ColorImage::from_rgba_unmultiplied(size, photo.as_raw()),
        Default::default(),
    ))
}
pub fn paint(
    p: &eframe::egui::Painter,
    texture: &eframe::egui::TextureHandle,
    center: eframe::egui::Pos2,
    radius: f32,
) {
    use eframe::egui::*;
    let mut mesh = epaint::Mesh::with_texture(texture.id());
    mesh.vertices.push(epaint::Vertex {
        pos: center,
        uv: pos2(0.5, 0.5),
        color: Color32::WHITE,
    });
    for i in 0..=64 {
        let angle = i as f32 * std::f32::consts::TAU / 64.;
        let offset = vec2(angle.cos(), angle.sin());
        mesh.vertices.push(epaint::Vertex {
            pos: center + radius * offset,
            uv: pos2(0.5 + offset.x * 0.5, 0.5 + offset.y * 0.5),
            color: Color32::WHITE,
        });
        if i > 0 {
            mesh.add_triangle(0, i, i + 1);
        }
    }
    p.add(mesh);
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_photo_is_center_cropped_and_bad_input_is_rejected() {
        let source =
            image::DynamicImage::ImageRgba8(image::RgbaImage::from_fn(400, 200, |x, _| {
                if x < 100 {
                    image::Rgba([255, 0, 0, 255])
                } else {
                    image::Rgba([0, 255, 0, 255])
                }
            }));
        let mut encoded = Cursor::new(Vec::new());
        source
            .write_to(&mut encoded, image::ImageFormat::Png)
            .unwrap();
        let bytes = prepare(encoded.get_ref()).unwrap();
        let image = image::load_from_memory(&bytes).unwrap().to_rgba8();
        assert_eq!(image.dimensions(), (256, 256));
        assert!(image.get_pixel(128, 128)[1] > 200);
        assert!(prepare(b"broken image").is_err());
        assert!(prepare(&vec![0; 16 * 1024 * 1024 + 1]).is_err());
    }
    #[test]
    fn jpeg_orientation_is_applied_before_cropping() {
        let photo = image::DynamicImage::ImageRgb8(image::RgbImage::from_fn(200, 100, |x, _| {
            if x < 100 {
                image::Rgb([240, 10, 10])
            } else {
                image::Rgb([10, 220, 10])
            }
        }));
        let mut encoded = Cursor::new(Vec::new());
        photo
            .write_to(&mut encoded, image::ImageFormat::Jpeg)
            .unwrap();
        let jpeg = encoded.into_inner();
        let exif = b"Exif\0\0II\x2a\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x06\0\0\0\0\0\0\0";
        let mut input = jpeg[..2].to_vec();
        input.extend_from_slice(&[0xff, 0xe1]);
        input.extend_from_slice(&((exif.len() + 2) as u16).to_be_bytes());
        input.extend_from_slice(exif);
        input.extend_from_slice(&jpeg[2..]);
        let output = image::load_from_memory(&prepare(&input).unwrap())
            .unwrap()
            .to_rgb8();
        assert!(output.get_pixel(128, 40)[0] > 200);
        assert!(output.get_pixel(128, 210)[1] > 180);
    }
}
