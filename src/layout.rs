use eframe::egui::{Rect, Vec2, pos2, vec2};
pub const HEADER_HEIGHT: f32 = 74.;
pub const ROW_HEIGHT: f32 = 68.;
pub const BUBBLE_PADDING: Vec2 = vec2(11., 5.);
pub const GROUP_GAP: f32 = 9.;
pub const MESSAGE_GAP: f32 = 2.;
pub const BODY_SIZE: f32 = 14.;
pub const SIDEBAR_INSET: f32 = 6.;
pub const THREAD_INSET: f32 = 20.;
pub fn composer_input(rect: Rect) -> Rect {
    Rect::from_min_max(rect.min + vec2(53., 8.), rect.max - vec2(52., 16.))
}
pub fn composer_text_width(main_width: f32) -> f32 {
    (main_width - 161.).max(100.)
}
pub fn bubble_size(text: Vec2) -> Vec2 {
    vec2(
        (text.x + BUBBLE_PADDING.x * 2.).max(30.),
        (text.y + BUBBLE_PADDING.y * 2.).max(27.),
    )
}
pub struct Regions {
    pub sidebar: Rect,
    pub main: Rect,
    pub header: Rect,
    pub thread: Rect,
    pub composer: Rect,
    pub error: Rect,
}
impl Regions {
    pub fn new(bounds: Rect, input_height: f32, has_error: bool) -> Self {
        let side_width = if bounds.width() < 900. { 264. } else { 294. };
        let sidebar = Rect::from_min_max(
            bounds.min + vec2(SIDEBAR_INSET, SIDEBAR_INSET),
            pos2(bounds.left() + side_width, bounds.bottom() - SIDEBAR_INSET),
        );
        let main = Rect::from_min_max(
            pos2(bounds.left() + side_width + 1., bounds.top() + 1.),
            bounds.max - vec2(1., 1.),
        );
        let header = Rect::from_min_size(main.min, vec2(main.width(), HEADER_HEIGHT));
        let composer = Rect::from_min_max(
            pos2(
                main.left(),
                main.bottom() - input_height.clamp(40., 128.) - 24.,
            ),
            main.max,
        );
        let error_height = if has_error { 44. } else { 0. };
        let error = Rect::from_min_max(
            pos2(main.left() + 54., composer.top() - error_height),
            pos2(main.right() - 52., composer.top()),
        );
        let thread = Rect::from_min_max(
            pos2(main.left() + THREAD_INSET, header.bottom()),
            pos2(main.right() - THREAD_INSET, composer.top() - error_height),
        );
        Self {
            sidebar,
            main,
            header,
            thread,
            composer,
            error,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn regions_do_not_overlap_at_supported_sizes() {
        for (w, h) in [(830., 570.), (1120., 760.), (1600., 1000.)] {
            for lines in [40., 128.] {
                for error in [false, true] {
                    let r =
                        Regions::new(Rect::from_min_size(pos2(0., 0.), vec2(w, h)), lines, error);
                    assert_eq!(r.thread.left() - r.main.left(), THREAD_INSET);
                    assert_eq!(r.main.right() - r.thread.right(), THREAD_INSET);
                    assert!(r.header.bottom() <= r.thread.top());
                    assert!(r.thread.bottom() <= r.composer.top());
                    assert!(r.thread.height() > 200.);
                    assert!(r.composer.bottom() <= h);
                }
            }
        }
    }
    #[test]
    fn bubbles_have_uniform_padding_and_minimum_height() {
        assert_eq!(bubble_size(vec2(60., 17.)), vec2(82., 27.));
        assert_eq!(bubble_size(vec2(60., 34.)), vec2(82., 44.));
        assert!(bubble_size(Vec2::ZERO).y >= 27.);
    }
    #[test]
    fn composer_measurement_matches_actual_editor_width() {
        for width in [565., 825., 1305.] {
            for height in [40., 80., 128.] {
                let rect = Rect::from_min_size(pos2(0., 0.), vec2(width, height + 24.));
                let input = composer_input(rect);
                assert_eq!(input.height(), height);
                assert_eq!(input.width() - 56., composer_text_width(width));
            }
        }
    }
}
