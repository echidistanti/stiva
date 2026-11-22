//! Look of the window: colours, text sizes and the few widgets egui does not draw the way this app wants.
//! The palette and the shapes come from Camallo; icons are drawn with shapes, so they do not depend
//! on which glyphs a font has.

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Margin, Painter, PointerButton, Pos2, Rect, ResizeDirection, Response, RichText, Sense,
    Shape, Stroke, StrokeKind, TextStyle, Ui, ViewportCommand, pos2, vec2,
};

pub const BG: Color32 = Color32::from_rgb(0x1a, 0x1c, 0x1e);
pub const PANEL: Color32 = Color32::from_rgb(0x22, 0x25, 0x27);
pub const CARD: Color32 = Color32::from_rgb(0x2a, 0x2d, 0x30);
pub const FIELD: Color32 = Color32::from_rgb(0x34, 0x38, 0x3b);
pub const LINE: Color32 = Color32::from_rgb(0x3b, 0x40, 0x44);
pub const TEXT: Color32 = Color32::from_rgb(0xe8, 0xea, 0xec);
pub const MUTED: Color32 = Color32::from_rgb(0x9b, 0xa1, 0xa7);
pub const TITLE: Color32 = Color32::from_rgb(0x0f, 0x10, 0x11);
/// Fill of the main buttons and of the progress bar.
pub const ACCENT: Color32 = Color32::from_rgb(0x8b, 0x5c, 0xf6);
/// Fill of the main buttons: darker than the accent, so white text on it reads at 5:1.
pub const ACCENT_FILL: Color32 = Color32::from_rgb(0x7c, 0x4d, 0xee);
/// The same fill under the pointer; the text on it stays above 4.5:1.
pub const ACCENT_FILL_HOVER: Color32 = Color32::from_rgb(0x85, 0x57, 0xf2);
/// The accent as text or outline on a dark background: lighter, so it stays readable.
pub const ACCENT_TEXT: Color32 = Color32::from_rgb(0xb9, 0xa3, 0xfb);
pub const ACCENT_DIM: Color32 = Color32::from_rgb(0x2b, 0x23, 0x48);
pub const ON_ACCENT: Color32 = Color32::WHITE;
pub const DANGER: Color32 = Color32::from_rgb(0xff, 0x6e, 0x6e);
pub const DANGER_DIM: Color32 = Color32::from_rgb(0x41, 0x25, 0x28);
/// A finished job.
pub const SUCCESS: Color32 = Color32::from_rgb(0x4e, 0xc3, 0x8b);
/// Something to look at that is not an error: an encoder that was not verified, a file that exists.
pub const WARN: Color32 = Color32::from_rgb(0xe3, 0xa6, 0x3c);

const HOVER: Color32 = Color32::from_rgb(0x42, 0x47, 0x4c);
const PRESSED: Color32 = Color32::from_rgb(0x4c, 0x52, 0x58);

/// Dark theme with one accent colour, rounded shapes and a bit more air than egui's default.
pub fn apply(ctx: &egui::Context) {
    ctx.set_theme(egui::ThemePreference::Dark);
    ctx.style_mut_of(egui::Theme::Dark, |s| {
        let v = &mut s.visuals;
        v.dark_mode = true;
        v.panel_fill = BG;
        v.window_fill = PANEL;
        v.window_stroke = Stroke::new(1.0, LINE);
        v.window_corner_radius = CornerRadius::same(14);
        v.menu_corner_radius = CornerRadius::same(10);
        v.extreme_bg_color = FIELD;
        v.faint_bg_color = CARD;
        v.hyperlink_color = ACCENT_TEXT;
        v.selection.bg_fill = ACCENT_DIM;
        v.selection.stroke = Stroke::new(1.0, ACCENT_TEXT);

        let r = CornerRadius::same(9);
        let w = &mut v.widgets;
        w.noninteractive.bg_fill = PANEL;
        w.noninteractive.weak_bg_fill = PANEL;
        w.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
        w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
        w.noninteractive.corner_radius = r;
        w.inactive.bg_fill = FIELD;
        w.inactive.weak_bg_fill = FIELD;
        w.inactive.bg_stroke = Stroke::NONE;
        w.inactive.fg_stroke = Stroke::new(1.0, TEXT);
        w.inactive.corner_radius = r;
        w.hovered.bg_fill = HOVER;
        w.hovered.weak_bg_fill = HOVER;
        w.hovered.bg_stroke = Stroke::new(1.0, MUTED);
        w.hovered.fg_stroke = Stroke::new(1.0, Color32::WHITE);
        w.hovered.corner_radius = r;
        w.active.bg_fill = PRESSED;
        w.active.weak_bg_fill = PRESSED;
        w.active.bg_stroke = Stroke::new(1.0, ACCENT_TEXT);
        w.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
        w.active.corner_radius = r;
        w.open.bg_fill = FIELD;
        w.open.weak_bg_fill = FIELD;
        w.open.bg_stroke = Stroke::new(1.0, MUTED);
        w.open.fg_stroke = Stroke::new(1.0, TEXT);
        w.open.corner_radius = r;

        s.spacing.item_spacing = vec2(8.0, 6.0);
        s.spacing.button_padding = vec2(12.0, 6.0);
        s.spacing.interact_size.y = 30.0;
        s.spacing.combo_width = 90.0;

        use egui::FontFamily::{Monospace, Proportional};
        s.text_styles = [
            (TextStyle::Heading, FontId::new(20.0, Proportional)),
            (TextStyle::Body, FontId::new(14.5, Proportional)),
            (TextStyle::Button, FontId::new(14.5, Proportional)),
            (TextStyle::Small, FontId::new(12.0, Proportional)),
            (TextStyle::Monospace, FontId::new(12.5, Monospace)),
        ]
        .into();
    });
}

/// Windows ships Segoe UI, which reads better than the bundled font; the bundled ones stay as fallback.
#[cfg_attr(not(windows), allow(unused_variables))]
pub fn fonts(ctx: &egui::Context) {
    #[cfg(windows)]
    {
        use std::sync::Arc;
        let dir = std::env::var_os("WINDIR").map(std::path::PathBuf::from).unwrap_or_else(|| "C:\\Windows".into()).join("Fonts");
        let mut defs = egui::FontDefinitions::default();
        let mut found = false;
        if let Ok(bytes) = std::fs::read(dir.join("segoeui.ttf")) {
            defs.font_data.insert("segoeui".into(), Arc::new(egui::FontData::from_owned(bytes)));
            defs.families.entry(egui::FontFamily::Proportional).or_default().insert(0, "segoeui".into());
            found = true;
        }
        if let Ok(bytes) = std::fs::read(dir.join("seguisym.ttf")) {
            defs.font_data.insert("seguisym".into(), Arc::new(egui::FontData::from_owned(bytes)));
            defs.families.entry(egui::FontFamily::Proportional).or_default().push("seguisym".into());
            found = true;
        }
        if found {
            ctx.set_fonts(defs);
        }
    }
}

pub fn muted(t: impl Into<String>) -> RichText {
    RichText::new(t.into()).color(MUTED).size(13.0)
}

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    /// The one action of the moment.
    Primary,
    Soft,
    /// No background until hovered.
    Ghost,
    /// A choice that is on.
    Selected,
    /// An action that destroys something: red, and never the one the eye is led to.
    Danger,
}

impl Kind {
    /// Text colour, fill, fill when hovered, outline.
    fn colors(self) -> (Color32, Color32, Color32, Stroke) {
        match self {
            Kind::Primary => (ON_ACCENT, ACCENT_FILL, ACCENT_FILL_HOVER, Stroke::NONE),
            Kind::Danger => (DANGER, DANGER_DIM, Color32::from_rgb(0x52, 0x2e, 0x31), Stroke::new(1.0, DANGER)),
            Kind::Soft => (TEXT, FIELD, HOVER, Stroke::NONE),
            Kind::Ghost => (MUTED, Color32::TRANSPARENT, FIELD, Stroke::NONE),
            Kind::Selected => (ACCENT_TEXT, ACCENT_DIM, Color32::from_rgb(0x38, 0x2d, 0x5e), Stroke::new(1.0, ACCENT)),
        }
    }
}

/// A rounded button. `size` is the text size, so the main action can be bigger.
pub fn pill(ui: &mut Ui, text: &str, kind: Kind, size: f32) -> Response {
    let (fg, fill, hover, stroke) = kind.colors();
    let galley = ui.painter().layout_no_wrap(text.to_owned(), FontId::proportional(size), fg);
    let pad = vec2(size * 0.95, size * 0.5);
    let (rect, resp) = ui.allocate_exact_size(galley.size() + pad * 2.0, Sense::click());
    if ui.is_rect_visible(rect) {
        let enabled = ui.is_enabled();
        let bg = if !enabled {
            fill.gamma_multiply(0.45)
        } else if resp.is_pointer_button_down_on() {
            hover.gamma_multiply(0.85)
        } else if resp.hovered() {
            hover
        } else {
            fill
        };
        let text_color = if kind == Kind::Ghost && resp.hovered() { TEXT } else { fg };
        ui.painter().rect(rect, CornerRadius::same(255), bg, stroke, StrokeKind::Inside);
        let at = rect.center() - galley.size() / 2.0;
        let text_color = if enabled { text_color } else { text_color.gamma_multiply(0.5) };
        ui.painter().galley_with_override_text_color(at, galley, text_color);
    }
    if resp.hovered() && ui.is_enabled() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// The queue button: the number of jobs in a badge, an accent outline while there are any, and a ring that
/// fades for a moment after one joined (`flash` from 1 down to 0), so the eye finds where it went.
pub fn queue_button(ui: &mut Ui, text: &str, count: usize, flash: f32) -> Response {
    let size = 15.0;
    let galley = ui.painter().layout_no_wrap(text.to_owned(), FontId::proportional(size), TEXT);
    let badge = (count > 0).then(|| ui.painter().layout_no_wrap(count.to_string(), FontId::proportional(13.0), ON_ACCENT));
    let pad = vec2(size * 0.95, size * 0.5);
    let badge_w = badge.as_ref().map_or(0.0, |b| (b.size().x + 12.0).max(22.0));
    let gap = if badge.is_some() { 8.0 } else { 0.0 };
    let size_all =
        vec2(pad.x * 2.0 + galley.size().x + gap + badge_w - if badge.is_some() { 4.0 } else { 0.0 }, galley.size().y + pad.y * 2.0);
    let (rect, resp) = ui.allocate_exact_size(size_all, Sense::click());
    if ui.is_rect_visible(rect) {
        let hot = resp.hovered() && ui.is_enabled();
        let base = if hot { HOVER } else { FIELD };
        let fill = lerp_color(base, ACCENT_DIM, flash);
        let stroke = if count > 0 { Stroke::new(1.5, ACCENT) } else { Stroke::NONE };
        ui.painter().rect(rect, CornerRadius::same(255), fill, stroke, StrokeKind::Inside);
        if flash > 0.0 {
            let ring = rect.expand(2.0 + 5.0 * (1.0 - flash));
            ui.painter().rect_stroke(
                ring,
                CornerRadius::same(255),
                Stroke::new(2.5, ACCENT_TEXT.gamma_multiply(flash)),
                StrokeKind::Middle,
            );
        }
        let at = pos2(rect.left() + pad.x, rect.center().y - galley.size().y / 2.0);
        ui.painter().galley_with_override_text_color(at, galley, TEXT);
        if let Some(b) = badge {
            let c = pos2(rect.right() - 6.0 - badge_w / 2.0, rect.center().y);
            ui.painter().rect_filled(Rect::from_center_size(c, vec2(badge_w, 22.0)), CornerRadius::same(255), ACCENT_FILL);
            ui.painter().galley_with_override_text_color(c - b.size() / 2.0, b, ON_ACCENT);
        }
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

/// Small button for the usual size of text.
pub fn button(ui: &mut Ui, text: &str, kind: Kind) -> Response {
    pill(ui, text, kind, 14.0)
}

/// A rounded box for one block of the window.
pub fn card<R>(ui: &mut Ui, fill: Color32, outline: Stroke, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .fill(fill)
        .stroke(outline)
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// A short label on a dark capsule: codec, size, resolution.
pub fn chip(ui: &mut Ui, text: &str) {
    egui::Frame::new().fill(FIELD).corner_radius(CornerRadius::same(255)).inner_margin(Margin::symmetric(8, 2)).show(ui, |ui| {
        ui.add(egui::Label::new(RichText::new(text).size(12.0).color(MUTED)).extend());
    });
}

fn cross_lines(p: &Painter, c: Pos2, d: f32, stroke: Stroke) {
    p.line_segment([c + vec2(-d, -d), c + vec2(d, d)], stroke);
    p.line_segment([c + vec2(-d, d), c + vec2(d, -d)], stroke);
}

/// A round "x" to close something.
pub fn close(ui: &mut Ui) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let hot = resp.hovered();
        if hot {
            ui.painter().circle_filled(rect.center(), 13.0, FIELD);
        }
        cross_lines(ui.painter(), rect.center(), 4.5, Stroke::new(1.6, if hot { TEXT } else { MUTED }));
        ui.ctx().set_cursor_icon(if hot { egui::CursorIcon::PointingHand } else { egui::CursorIcon::Default });
    }
    resp
}

/// Thin progress bar. Without a fraction a short segment slides back and forth.
pub fn bar(ui: &mut Ui, fraction: Option<f32>, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 6.0), Sense::hover());
    let r = CornerRadius::same(3);
    ui.painter().rect_filled(rect, r, LINE);
    match fraction {
        Some(f) => {
            let w = rect.width() * f.clamp(0.0, 1.0);
            if w > 0.5 {
                ui.painter().rect_filled(Rect::from_min_size(rect.min, vec2(w.max(6.0), rect.height())), r, color);
            }
        }
        None => {
            let t = ui.input(|i| i.time) as f32;
            let phase = (t * 0.8).fract();
            let pos = if phase < 0.5 { phase * 2.0 } else { (1.0 - phase) * 2.0 };
            let w = rect.width() * 0.2;
            let x = rect.left() + (rect.width() - w) * pos;
            ui.painter().rect_filled(Rect::from_min_size(pos2(x, rect.top()), vec2(w, rect.height())), r, color);
            ui.ctx().request_repaint();
        }
    }
}

/// Two stacked containers in outline, for an empty source.
pub fn empty_icon(ui: &mut Ui, hot: bool) {
    let (rect, _) = ui.allocate_exact_size(vec2(84.0, 70.0), Sense::hover());
    let p = ui.painter();
    let c = rect.center();
    let ink = Stroke::new(2.5, if hot { ACCENT_TEXT } else { LINE });
    let low = Rect::from_center_size(c + vec2(0.0, 14.0), vec2(72.0, 28.0));
    let high = Rect::from_center_size(c + vec2(0.0, -14.0), vec2(54.0, 24.0));
    for (r, n) in [(low, 6), (high, 4)] {
        p.rect_stroke(r, CornerRadius::same(6), ink, StrokeKind::Middle);
        for k in 0..n {
            let x = r.left() + r.width() * (k as f32 + 1.0) / (n as f32 + 1.0);
            p.line_segment([pos2(x, r.top() + 7.0), pos2(x, r.bottom() - 7.0)], Stroke::new(2.0, ink.color));
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Icon {
    Folder,
    Close,
    Minimize,
    Maximize,
    Restore,
}

/// An icon about 16 px wide around `c`, drawn with lines.
pub fn draw_icon(p: &Painter, c: Pos2, icon: Icon, color: Color32) {
    let st = |w: f32| Stroke::new(w, color);
    let at = |x: f32, y: f32| c + vec2(x, y);
    match icon {
        Icon::Close => cross_lines(p, c, 5.0, st(1.6)),
        Icon::Minimize => {
            p.line_segment([at(-5.0, 0.0), at(5.0, 0.0)], st(1.3));
        }
        Icon::Maximize => {
            p.rect_stroke(Rect::from_center_size(c, vec2(10.0, 10.0)), CornerRadius::ZERO, st(1.3), StrokeKind::Middle);
        }
        Icon::Restore => {
            p.rect_stroke(Rect::from_center_size(at(-1.0, 1.0), vec2(8.0, 8.0)), CornerRadius::ZERO, st(1.3), StrokeKind::Middle);
            p.add(Shape::line(vec![at(-2.0, -5.0), at(5.0, -5.0), at(5.0, 2.0)], st(1.3)));
        }
        Icon::Folder => {
            p.add(Shape::closed_line(
                vec![at(-8.0, -5.5), at(-3.0, -5.5), at(-1.0, -3.0), at(8.0, -3.0), at(8.0, 6.0), at(-8.0, 6.0)],
                st(1.7),
            ));
        }
    }
}

/// A round button with an icon.
pub fn icon_button(ui: &mut Ui, icon: Icon, on: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(34.0, 34.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let enabled = ui.is_enabled();
        let hot = resp.hovered() && enabled;
        if on {
            ui.painter().circle_filled(rect.center(), 17.0, ACCENT_DIM);
        } else if hot {
            ui.painter().circle_filled(rect.center(), 17.0, FIELD);
        }
        let color = match (on, hot) {
            (true, _) => ACCENT_TEXT,
            (false, true) => TEXT,
            _ => MUTED,
        };
        draw_icon(ui.painter(), rect.center(), icon, if enabled { color } else { color.gamma_multiply(0.4) });
    }
    if resp.hovered() && ui.is_enabled() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// A button of the title bar. `danger` turns red when hovered, like the close button of Windows.
fn window_button(ui: &mut Ui, icon: Icon, danger: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(46.0, ui.available_height()), Sense::click());
    if resp.hovered() {
        let fill = if danger { Color32::from_rgb(0xe8, 0x11, 0x23) } else { Color32::from_rgb(0x2a, 0x2c, 0x2f) };
        ui.painter().rect_filled(rect, CornerRadius::ZERO, fill);
    }
    let color = if resp.hovered() { Color32::WHITE } else { MUTED };
    draw_icon(ui.painter(), rect.center(), icon, color);
    resp
}

pub const TITLE_HEIGHT: f32 = 34.0;

/// The window has no frame of the system, so this bar does what the frame did: name, move, minimise, maximise, close.
pub fn title_bar(ui: &mut Ui, name: &str, minimize: &str, maximize: &str, restore: &str) {
    let ctx = ui.ctx().clone();
    let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
    let bar = ui.max_rect();
    // Added first, so the buttons on top of it win the pointer where they overlap.
    let drag = ui.interact(bar, ui.scope_id().with("title_drag"), Sense::click_and_drag());
    if drag.drag_started_by(PointerButton::Primary) {
        ctx.send_viewport_cmd(ViewportCommand::StartDrag);
    }
    if drag.double_clicked() {
        ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
    }
    let p = ui.painter();
    // The icon of the program in small: a violet tile with two stacked containers.
    let mid = pos2(bar.left() + 25.0, bar.center().y);
    p.rect_filled(Rect::from_center_size(mid, vec2(16.0, 16.0)), CornerRadius::same(5), ACCENT);
    p.rect_filled(Rect::from_center_size(mid + vec2(0.0, 2.6), vec2(11.0, 4.6)), CornerRadius::same(1), ON_ACCENT);
    p.rect_filled(
        Rect::from_center_size(mid + vec2(0.0, -2.6), vec2(8.0, 4.0)),
        CornerRadius::same(1),
        Color32::from_rgb(0xed, 0xe9, 0xfe),
    );
    p.text(pos2(bar.left() + 40.0, bar.center().y), Align2::LEFT_CENTER, name, FontId::proportional(13.0), MUTED);

    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        if window_button(ui, Icon::Close, true).clicked() {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
        let (icon, tip) = if maximized { (Icon::Restore, restore) } else { (Icon::Maximize, maximize) };
        if window_button(ui, icon, false).on_hover_text(tip).clicked() {
            ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
        }
        if window_button(ui, Icon::Minimize, false).on_hover_text(minimize).clicked() {
            ctx.send_viewport_cmd(ViewportCommand::Minimized(true));
        }
    });
}

/// Which edge or corner of the window a point is on, within `m` pixels of the border.
pub fn edge_at(p: Pos2, r: Rect, m: f32) -> Option<ResizeDirection> {
    use ResizeDirection::*;
    let (l, rt, t, b) = (p.x - r.left() < m, r.right() - p.x < m, p.y - r.top() < m, r.bottom() - p.y < m);
    match (l, rt, t, b) {
        (true, _, true, _) => Some(NorthWest),
        (_, true, true, _) => Some(NorthEast),
        (true, _, _, true) => Some(SouthWest),
        (_, true, _, true) => Some(SouthEast),
        (true, ..) => Some(West),
        (_, true, ..) => Some(East),
        (_, _, true, _) => Some(North),
        (_, _, _, true) => Some(South),
        _ => None,
    }
}

fn resize_cursor(d: ResizeDirection) -> egui::CursorIcon {
    use ResizeDirection::*;
    use egui::CursorIcon as C;
    match d {
        North => C::ResizeNorth,
        South => C::ResizeSouth,
        East => C::ResizeEast,
        West => C::ResizeWest,
        NorthEast => C::ResizeNorthEast,
        NorthWest => C::ResizeNorthWest,
        SouthEast => C::ResizeSouthEast,
        SouthWest => C::ResizeSouthWest,
    }
}

/// Without the system frame the window cannot be resized from its border unless this does it.
pub fn resize_edges(ctx: &egui::Context) {
    let (maximized, pos) = ctx.input(|i| (i.viewport().maximized.unwrap_or(false), i.pointer.hover_pos()));
    let Some(pos) = pos.filter(|_| !maximized) else {
        return;
    };
    if let Some(dir) = edge_at(pos, ctx.content_rect(), 6.0) {
        ctx.set_cursor_icon(resize_cursor(dir));
        if ctx.input(|i| i.pointer.primary_pressed()) {
            ctx.send_viewport_cmd(ViewportCommand::BeginResize(dir));
        }
    }
}

/// A thin line around the window, since there is no frame to show where it ends.
pub fn border(ctx: &egui::Context) {
    ctx.layer_painter(egui::LayerId::debug()).rect_stroke(
        ctx.content_rect(),
        CornerRadius::ZERO,
        Stroke::new(1.0, LINE),
        StrokeKind::Inside,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::pos2;

    #[test]
    fn border_zones_name_the_edge_or_corner() {
        use ResizeDirection::*;
        let r = Rect::from_min_size(pos2(0.0, 0.0), vec2(100.0, 80.0));
        assert_eq!(edge_at(pos2(50.0, 40.0), r, 6.0), None);
        assert_eq!(edge_at(pos2(2.0, 40.0), r, 6.0), Some(West));
        assert_eq!(edge_at(pos2(98.0, 40.0), r, 6.0), Some(East));
        assert_eq!(edge_at(pos2(50.0, 1.0), r, 6.0), Some(North));
        assert_eq!(edge_at(pos2(50.0, 79.0), r, 6.0), Some(South));
        assert_eq!(edge_at(pos2(1.0, 1.0), r, 6.0), Some(NorthWest));
        assert_eq!(edge_at(pos2(99.0, 1.0), r, 6.0), Some(NorthEast));
        assert_eq!(edge_at(pos2(1.0, 79.0), r, 6.0), Some(SouthWest));
        assert_eq!(edge_at(pos2(99.0, 79.0), r, 6.0), Some(SouthEast));
        assert_eq!(edge_at(pos2(7.0, 40.0), r, 6.0), None, "just inside the zone");
    }
}
