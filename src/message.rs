//! The on-screen message box (Rebuild Prompt, "Controls, Message, And
//! Responsive Behavior").
//!
//! Three parts, none of which touch the GPU:
//!
//! - [`parse_message`] validates `--message`: surrounding whitespace is
//!   trimmed, empty text hides the box, and the limit is 240 UTF-16 code
//!   units (supplementary characters count as two).
//! - [`Notice`] decides what the box shows. The launch message is shown
//!   steadily; a palette change briefly shows the palette's name instead,
//!   then the message comes back (owner decision, 2026-11-03).
//! - [`render_message_box`] lays the text out and rasterizes the box at
//!   device resolution as premultiplied, sRGB-encoded RGBA. The shader
//!   blends it over the city in sRGB space, as a browser would.
//!
//! The box is centered at 50% width and 42% height, at most 640 CSS pixels
//! wide with 16 CSS pixels of side clearance. Its background is the palette
//! `bg` at 75% opacity, the text is the palette `star` color, and the border
//! is halfway between the two. Text wraps at spaces, and words too long for
//! a line break between characters. Text is literal: nothing is interpreted
//! as markup.
//!
//! Glyphs come from Noto Sans Regular (SIL Open Font License 1.1, see
//! `src/fonts/NotoSans-OFL.txt`), compiled into the binary. Characters the
//! font lacks (emoji, CJK) draw as the font's missing-glyph box. There is no
//! text shaping: combining marks are placed by their own metrics, which suits
//! Latin, Greek, and Cyrillic.

use std::sync::OnceLock;

use fontdue::{Font, FontSettings};

use crate::model::palette::{Palette, Rgb};

/// Longest message, in UTF-16 code units (Rebuild Prompt).
pub const MESSAGE_LIMIT_UTF16: usize = 240;
/// How long a palette change shows the palette's name, in seconds.
pub const PALETTE_NOTICE_SECONDS: f64 = 2.0;

/// Box geometry in CSS pixels; multiplied by the display scale factor.
const FONT_PX: f64 = 24.0;
const PADDING_X: f64 = 18.0;
const PADDING_Y: f64 = 10.0;
const BORDER: f64 = 2.0;
const MAX_WIDTH: f64 = 640.0;
const SIDE_CLEARANCE: f64 = 16.0;
/// Box center as a fraction of the surface (Rebuild Prompt: 50% / 42%).
const CENTER: (f64, f64) = (0.5, 0.42);
/// Background opacity (Rebuild Prompt: palette `bg` at 75%).
const BACKGROUND_ALPHA: f64 = 0.75;

static FONT_BYTES: &[u8] = include_bytes!("fonts/NotoSans-Regular.ttf");

/// The bundled message font, parsed once.
///
/// # Panics
///
/// Panics if the bundled font fails to parse, which a unit test rules out.
pub fn message_font() -> &'static Font {
    static FONT: OnceLock<Font> = OnceLock::new();
    FONT.get_or_init(|| {
        Font::from_bytes(FONT_BYTES, FontSettings::default())
            .expect("the bundled Noto Sans font parses")
    })
}

/// Validates `--message` text.
///
/// Returns `Ok(None)` for empty or whitespace-only text, which hides the box.
///
/// # Errors
///
/// Returns a message when the trimmed text is longer than
/// [`MESSAGE_LIMIT_UTF16`] UTF-16 code units.
pub fn parse_message(raw: &str) -> Result<Option<String>, String> {
    let text = raw.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let units = text.encode_utf16().count();
    if units > MESSAGE_LIMIT_UTF16 {
        return Err(format!(
            "--message is {units} UTF-16 units long; the limit is {MESSAGE_LIMIT_UTF16}"
        ));
    }
    Ok(Some(text.to_string()))
}

/// What the message box shows, highest priority first: a palette's name
/// briefly after a change, the launch message, then a status line (the
/// Endless startup quips). Times are seconds on any monotonic clock.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Notice {
    message: Option<String>,
    /// A temporary text and the time it stops showing.
    flash: Option<(String, f64)>,
    /// Shown only when there is no message and no flash.
    status: Option<String>,
}

impl Notice {
    /// A notice showing `message` steadily (`None` hides the box).
    pub fn new(message: Option<String>) -> Self {
        Self {
            message,
            flash: None,
            status: None,
        }
    }

    /// Sets or clears the status line. A launch message hides it, so a
    /// meeting screen keeps its message (agent default, 2026-11-03).
    pub fn set_status(&mut self, status: Option<&str>) {
        if self.status.as_deref() != status {
            self.status = status.map(str::to_string);
        }
    }

    /// Shows `text` from `now` for [`PALETTE_NOTICE_SECONDS`], replacing any
    /// earlier flash; the message returns afterwards.
    pub fn flash(&mut self, text: &str, now: f64) {
        self.flash = Some((text.to_string(), now + PALETTE_NOTICE_SECONDS));
    }

    /// The text to show at `now`, or `None` to hide the box.
    pub fn text(&self, now: f64) -> Option<&str> {
        match &self.flash {
            Some((text, until)) if now < *until => Some(text),
            _ => self.message.as_deref().or(self.status.as_deref()),
        }
    }
}

/// A rasterized message box: premultiplied, sRGB-encoded RGBA rows, placed
/// at `(x, y)` in device pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct MessageImage {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Box sizes in device pixels for one scale factor.
#[derive(Debug, Clone, Copy)]
struct Metrics {
    font_px: f32,
    padding_x: usize,
    padding_y: usize,
    border: usize,
    max_width: usize,
    clearance: usize,
}

impl Metrics {
    fn new(scale: f64) -> Self {
        let scale = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };
        let px = |value: f64| ((value * scale).round() as usize).max(1);
        Self {
            font_px: (FONT_PX * scale) as f32,
            padding_x: px(PADDING_X),
            padding_y: px(PADDING_Y),
            border: px(BORDER),
            max_width: px(MAX_WIDTH),
            clearance: px(SIDE_CLEARANCE),
        }
    }

    /// Border plus padding on one side, horizontally and vertically.
    fn inset(&self) -> (usize, usize) {
        (self.border + self.padding_x, self.border + self.padding_y)
    }
}

/// Lays out and rasterizes `text` in a box for a `surface` of device pixels.
///
/// Returns `None` when the text is empty or the surface is too small to hold
/// even one line. Lines that don't fit the surface height are left out.
///
/// Time: O(characters × glyph area). It runs only when the text, palette,
/// surface size, or scale changes, never per frame.
pub fn render_message_box(
    font: &Font,
    text: &str,
    surface: (u32, u32),
    scale: f64,
    palette: &Palette,
) -> Option<MessageImage> {
    if text.trim().is_empty() {
        return None;
    }
    let metrics = Metrics::new(scale);
    let (surface_w, surface_h) = (surface.0 as usize, surface.1 as usize);
    let (inset_x, inset_y) = metrics.inset();
    let box_limit = metrics
        .max_width
        .min(surface_w.saturating_sub(2 * metrics.clearance));
    let wrap_width = box_limit.checked_sub(2 * inset_x).filter(|w| *w > 0)?;
    let measure = |c: char| font.metrics(c, metrics.font_px).advance_width;
    let mut lines = wrap_text(text, wrap_width as f32, measure);

    let line_metrics = font.horizontal_line_metrics(metrics.font_px)?;
    let line_height = line_metrics.new_line_size.ceil() as usize;
    let height_limit = surface_h.saturating_sub(2 * metrics.clearance);
    let max_lines = height_limit.saturating_sub(2 * inset_y) / line_height.max(1);
    if max_lines == 0 {
        return None;
    }
    lines.truncate(max_lines);

    let widest = lines
        .iter()
        .map(|line| line_width(line, measure))
        .fold(0.0_f32, f32::max);
    let width = (widest.ceil() as usize + 2 * inset_x).min(box_limit);
    let height = lines.len() * line_height + 2 * inset_y;

    // Text coverage, then one compositing pass over the whole box.
    let mut coverage = vec![0_u8; width * height];
    for (index, line) in lines.iter().enumerate() {
        let line_w = line_width(line, measure);
        let left = inset_x as f32 + ((width - 2 * inset_x) as f32 - line_w) / 2.0;
        let baseline = (inset_y + index * line_height) as f32 + line_metrics.ascent;
        draw_line(
            font,
            metrics.font_px,
            line,
            left,
            baseline,
            (width, height),
            &mut coverage,
        );
    }
    let rgba = composite(&coverage, (width, height), metrics.border, palette);

    let x = ((surface_w as f64 * CENTER.0) - width as f64 / 2.0).round();
    let y = ((surface_h as f64 * CENTER.1) - height as f64 / 2.0).round();
    let x = (x.max(0.0) as usize).min(surface_w.saturating_sub(width));
    let y = (y.max(0.0) as usize).min(surface_h.saturating_sub(height));
    Some(MessageImage {
        x: x as u32,
        y: y as u32,
        width: width as u32,
        height: height as u32,
        rgba,
    })
}

/// Splits `text` into lines no wider than `max_width`, measured by summing
/// each character's advance from `measure`.
///
/// Newlines force a break. Spaces between words are kept single at line
/// joins and dropped at breaks. A word wider than a line is broken between
/// characters, but never before a combining mark or joiner.
///
/// Time: O(n) in the number of characters.
pub fn wrap_text<F>(text: &str, max_width: f32, measure: F) -> Vec<String>
where
    F: Fn(char) -> f32,
{
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        let mut line_w = 0.0_f32;
        for word in paragraph.split_whitespace() {
            let word_w = line_width(word, &measure);
            let space_w = if line.is_empty() { 0.0 } else { measure(' ') };
            if line_w + space_w + word_w <= max_width {
                if !line.is_empty() {
                    line.push(' ');
                }
                line.push_str(word);
                line_w += space_w + word_w;
                continue;
            }
            if !line.is_empty() {
                lines.push(std::mem::take(&mut line));
                line_w = 0.0;
            }
            if word_w <= max_width {
                line.push_str(word);
                line_w = word_w;
                continue;
            }
            for cluster in clusters(word) {
                let cluster_w = line_width(cluster, &measure);
                if !line.is_empty() && line_w + cluster_w > max_width {
                    lines.push(std::mem::take(&mut line));
                    line_w = 0.0;
                }
                line.push_str(cluster);
                line_w += cluster_w;
            }
        }
        if !line.is_empty() {
            lines.push(line);
        }
    }
    lines
}

/// The width of `text` as a sum of character advances.
fn line_width<F>(text: &str, measure: F) -> f32
where
    F: Fn(char) -> f32,
{
    text.chars().map(measure).sum()
}

/// Splits a word into base characters with their trailing combining marks,
/// joiners, and variation selectors, so a forced break never separates them.
fn clusters(word: &str) -> Vec<&str> {
    let mut result = Vec::new();
    let mut start = 0;
    for (index, c) in word.char_indices().skip(1) {
        if !is_attached(c) {
            result.push(&word[start..index]);
            start = index;
        }
    }
    result.push(&word[start..]);
    result
}

/// True for characters that attach to the one before them.
fn is_attached(c: char) -> bool {
    matches!(
        c,
        '\u{0300}'..='\u{036F}'
            | '\u{1AB0}'..='\u{1AFF}'
            | '\u{1DC0}'..='\u{1DFF}'
            | '\u{20D0}'..='\u{20FF}'
            | '\u{FE20}'..='\u{FE2F}'
            | '\u{FE00}'..='\u{FE0F}'
            | '\u{200D}'
    )
}

/// Adds one line's glyph coverage to `coverage`, saturating at full.
fn draw_line(
    font: &Font,
    px: f32,
    line: &str,
    left: f32,
    baseline: f32,
    (width, height): (usize, usize),
    coverage: &mut [u8],
) {
    let mut pen = left;
    for c in line.chars() {
        let (glyph, bitmap) = font.rasterize(c, px);
        let gx = (pen + glyph.xmin as f32).round() as i64;
        let gy = (baseline - (glyph.height as i32 + glyph.ymin) as f32).round() as i64;
        for row in 0..glyph.height {
            let y = gy + row as i64;
            if y < 0 || y >= height as i64 {
                continue;
            }
            for col in 0..glyph.width {
                let x = gx + col as i64;
                if x < 0 || x >= width as i64 {
                    continue;
                }
                let target = &mut coverage[y as usize * width + x as usize];
                *target = target.saturating_add(bitmap[row * glyph.width + col]);
            }
        }
        pen += glyph.advance_width;
    }
}

/// Builds the box image: border, translucent background, and text over it,
/// premultiplied in sRGB-encoded values.
fn composite(
    coverage: &[u8],
    (width, height): (usize, usize),
    border: usize,
    palette: &Palette,
) -> Vec<u8> {
    let bg = palette.bg.0.map(f64::from);
    let star = palette.star.0.map(f64::from);
    let edge = border_color(palette).0.map(f64::from);
    let mut rgba = Vec::with_capacity(width * height * 4);
    for y in 0..height {
        for x in 0..width {
            let on_border = x < border || y < border || x + border >= width || y + border >= height;
            let (base, base_alpha) = if on_border {
                (edge, 1.0)
            } else {
                (bg, BACKGROUND_ALPHA)
            };
            // Text over the box background ("over" compositing).
            let ink = f64::from(coverage[y * width + x]) / 255.0;
            let alpha = ink + base_alpha * (1.0 - ink);
            for channel in 0..3 {
                let value = star[channel] * ink + base[channel] * base_alpha * (1.0 - ink);
                rgba.push(value.round().clamp(0.0, 255.0) as u8);
            }
            rgba.push((alpha * 255.0).round() as u8);
        }
    }
    rgba
}

/// The border color: halfway between the palette's star and background.
pub fn border_color(palette: &Palette) -> Rgb {
    let mix = |a: u8, b: u8| (u16::from(a) + u16::from(b)).div_ceil(2) as u8;
    let (star, bg) = (palette.star.0, palette.bg.0);
    Rgb([
        mix(star[0], bg[0]),
        mix(star[1], bg[1]),
        mix(star[2], bg[2]),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::palette::{palette_by_key, palettes};

    const SURFACE: (u32, u32) = (1920, 1080);

    fn classic() -> &'static Palette {
        palette_by_key("classic").unwrap()
    }

    fn render(text: &str) -> MessageImage {
        render_message_box(message_font(), text, SURFACE, 1.0, classic()).expect("a box")
    }

    fn pixel(image: &MessageImage, x: u32, y: u32) -> [u8; 4] {
        let at = ((y * image.width + x) * 4) as usize;
        image.rgba[at..at + 4].try_into().unwrap()
    }

    fn ink_pixels(image: &MessageImage, palette: &Palette) -> usize {
        let star = palette.star.0;
        image
            .rgba
            .chunks(4)
            .filter(|p| p[3] == 255 && p[..3] == star)
            .count()
    }

    #[test]
    fn parse_message_trims_and_hides_blank_text() {
        assert_eq!(
            parse_message("  Starting soon \n"),
            Ok(Some("Starting soon".into()))
        );
        assert_eq!(parse_message(""), Ok(None));
        assert_eq!(parse_message(" \t\n "), Ok(None));
    }

    #[test]
    fn parse_message_keeps_markup_literal() {
        let text = "<b>bold</b> & <script>x</script>";
        assert_eq!(parse_message(text), Ok(Some(text.into())));
    }

    #[test]
    fn parse_message_limit_counts_utf16_units() {
        let at_limit = "a".repeat(MESSAGE_LIMIT_UTF16);
        assert!(parse_message(&at_limit).unwrap().is_some());
        assert!(parse_message(&format!("{at_limit}b")).is_err());
        // A supplementary character is two UTF-16 units.
        let emoji = "\u{1F319}".repeat(MESSAGE_LIMIT_UTF16 / 2);
        assert!(parse_message(&emoji).unwrap().is_some());
        assert!(parse_message(&format!("{emoji}a")).is_err());
        // Surrounding whitespace does not count.
        assert!(parse_message(&format!("   {at_limit}   "))
            .unwrap()
            .is_some());
    }

    #[test]
    fn notice_shows_the_message_steadily() {
        let notice = Notice::new(Some("Hello".into()));
        assert_eq!(notice.text(0.0), Some("Hello"));
        assert_eq!(notice.text(1e6), Some("Hello"));
        assert_eq!(Notice::new(None).text(0.0), None);
    }

    #[test]
    fn palette_flash_replaces_then_restores_the_message() {
        let mut notice = Notice::new(Some("Meeting soon".into()));
        notice.flash("ArtDeco", 10.0);
        assert_eq!(notice.text(10.0), Some("ArtDeco"));
        assert_eq!(
            notice.text(10.0 + PALETTE_NOTICE_SECONDS - 0.01),
            Some("ArtDeco")
        );
        assert_eq!(
            notice.text(10.0 + PALETTE_NOTICE_SECONDS),
            Some("Meeting soon")
        );
    }

    #[test]
    fn palette_flash_without_a_message_hides_afterwards() {
        let mut notice = Notice::new(None);
        notice.flash("EGA", 0.0);
        assert_eq!(notice.text(1.0), Some("EGA"));
        assert_eq!(notice.text(PALETTE_NOTICE_SECONDS + 0.1), None);
    }

    #[test]
    fn status_shows_only_without_a_message_and_yields_to_a_flash() {
        let mut quiet = Notice::new(None);
        quiet.set_status(Some("Polishing the moon"));
        assert_eq!(quiet.text(0.0), Some("Polishing the moon"));
        quiet.flash("EGA", 1.0);
        assert_eq!(quiet.text(1.5), Some("EGA"));
        assert_eq!(
            quiet.text(1.0 + PALETTE_NOTICE_SECONDS),
            Some("Polishing the moon")
        );
        quiet.set_status(None);
        assert_eq!(quiet.text(10.0), None);

        let mut meeting = Notice::new(Some("Standup at 10".into()));
        meeting.set_status(Some("Herding taxis"));
        assert_eq!(meeting.text(0.0), Some("Standup at 10"));
    }

    #[test]
    fn a_new_flash_restarts_the_timer() {
        let mut notice = Notice::new(Some("m".into()));
        notice.flash("VGA", 0.0);
        notice.flash("Commodore 64", 1.5);
        assert_eq!(notice.text(2.5), Some("Commodore 64"));
        assert_eq!(notice.text(1.5 + PALETTE_NOTICE_SECONDS), Some("m"));
    }

    #[test]
    fn wrap_breaks_at_spaces_within_the_width() {
        let unit = |_: char| 1.0;
        assert_eq!(
            wrap_text("aaa bbb ccc", 7.0, unit),
            vec!["aaa bbb".to_string(), "ccc".to_string()]
        );
        assert_eq!(wrap_text("  a   b  ", 10.0, unit), vec!["a b".to_string()]);
        assert!(wrap_text("", 10.0, unit).is_empty());
    }

    #[test]
    fn wrap_honors_newlines_and_breaks_long_words() {
        let unit = |_: char| 1.0;
        assert_eq!(
            wrap_text("ab\ncd", 10.0, unit),
            vec!["ab".to_string(), "cd".to_string()]
        );
        assert_eq!(
            wrap_text("abcdefgh", 3.0, unit),
            vec!["abc".to_string(), "def".to_string(), "gh".to_string()]
        );
    }

    #[test]
    fn wrap_never_separates_a_combining_mark_from_its_base() {
        // "e" + combining acute counts as one cluster of width 1.
        let measure = |c: char| if is_attached(c) { 0.0 } else { 1.0 };
        let word = "ae\u{0301}io";
        let lines = wrap_text(word, 2.0, measure);
        assert_eq!(lines, vec!["ae\u{0301}".to_string(), "io".to_string()]);
    }

    #[test]
    fn bundled_font_parses_and_covers_latin_greek_cyrillic() {
        let font = message_font();
        for c in ['A', 'z', 'é', 'ß', 'Ω', 'Ж', '&', '<'] {
            assert!(font.has_glyph(c), "missing {c}");
        }
    }

    #[test]
    fn box_is_centered_at_half_width_and_42_percent_height() {
        let image = render("This meeting will start shortly");
        let center_x = f64::from(image.x) + f64::from(image.width) / 2.0;
        let center_y = f64::from(image.y) + f64::from(image.height) / 2.0;
        assert!((center_x - 960.0).abs() <= 1.0, "{center_x}");
        assert!((center_y - 1080.0 * 0.42).abs() <= 1.0, "{center_y}");
    }

    #[test]
    fn box_uses_palette_colors_and_75_percent_background() {
        for palette in palettes() {
            let image =
                render_message_box(message_font(), "Hi", SURFACE, 1.0, palette).expect("a box");
            let edge = border_color(palette).0;
            assert_eq!(pixel(&image, 0, 0), [edge[0], edge[1], edge[2], 255]);
            // Inside the border, away from the text: premultiplied bg at 75%.
            let inside = pixel(&image, 3, 3);
            assert_eq!(inside[3], 191, "{}", palette.key);
            for (value, bg) in inside.iter().zip(palette.bg.0) {
                let expected = (f64::from(bg) * 0.75).round() as u8;
                assert_eq!(*value, expected, "{}", palette.key);
            }
            assert!(ink_pixels(&image, palette) > 0, "{}", palette.key);
        }
    }

    #[test]
    fn border_is_halfway_between_star_and_background() {
        let parchment = palette_by_key("blackonwhite").unwrap();
        let star = parchment.star.0;
        let bg = parchment.bg.0;
        let edge = border_color(parchment).0;
        for channel in 0..3 {
            let mid = (u16::from(star[channel]) + u16::from(bg[channel])) as f64 / 2.0;
            assert!((f64::from(edge[channel]) - mid).abs() <= 0.5);
        }
    }

    #[test]
    fn long_text_wraps_within_640_css_pixels() {
        let text = "word ".repeat(48);
        let image = render(&text);
        assert!(image.width <= 640);
        let one_line = render("word");
        assert!(image.height >= 3 * (one_line.height - 2 * (2 + 10)));
        // Twice the scale doubles the limit.
        let big = render_message_box(message_font(), &text, (3840, 2160), 2.0, classic()).unwrap();
        assert!(big.width <= 1280 && big.width > 640);
    }

    #[test]
    fn narrow_surfaces_keep_side_clearance() {
        let image =
            render_message_box(message_font(), &"x".repeat(200), (320, 180), 1.0, classic())
                .expect("a box");
        assert!(image.x >= 16);
        assert!(image.x + image.width <= 320 - 16);
        assert!(image.y + image.height <= 180);
    }

    #[test]
    fn tall_text_is_cut_to_the_surface_height() {
        let text = "x\n".repeat(100);
        let image = render_message_box(message_font(), &text, (640, 200), 1.0, classic()).unwrap();
        assert!(image.height <= 200 - 32);
    }

    #[test]
    fn blank_text_or_a_tiny_surface_draws_nothing() {
        let font = message_font();
        assert!(render_message_box(font, "  ", SURFACE, 1.0, classic()).is_none());
        assert!(render_message_box(font, "Hi", (40, 40), 1.0, classic()).is_none());
    }

    #[test]
    fn markup_is_drawn_as_literal_characters() {
        let plain = render("b");
        let tagged = render("<b>");
        assert!(tagged.width > plain.width);
        assert!(ink_pixels(&tagged, classic()) > ink_pixels(&plain, classic()));
    }

    #[test]
    fn supplementary_and_combining_characters_render() {
        let image = render("Caf\u{0065}\u{0301} \u{1F319}");
        assert!(ink_pixels(&image, classic()) > 0);
    }

    #[test]
    fn same_inputs_give_the_same_image() {
        assert_eq!(render("Repeatable"), render("Repeatable"));
    }
}
