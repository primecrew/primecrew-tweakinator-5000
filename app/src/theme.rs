//! Look & feel: System / Dark / Light / High-contrast, semantic colour tokens,
//! typography, spacing, text-size zoom and a keyboard focus ring.
//! Semantic text tokens follow WCAG AA (>= 4.5:1) on the backgrounds they are drawn on
//! (see `contrast` and the tests). Inspired by the FFMPEG Encoder app's theme design notes.
use eframe::egui::{self, Color32, FontFamily, FontId, Stroke, TextStyle, Theme, ThemePreference, Visuals};
use std::cell::Cell;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Look {
    System,
    Dark,
    Light,
    HighContrast,
}

impl Look {
    pub const ALL: [Look; 4] = [Look::System, Look::Dark, Look::Light, Look::HighContrast];
    pub fn label(self) -> &'static str {
        match self {
            Look::System => "System",
            Look::Dark => "Dark",
            Look::Light => "Light",
            Look::HighContrast => "High contrast",
        }
    }
    fn key(self) -> &'static str {
        match self {
            Look::System => "system",
            Look::Dark => "dark",
            Look::Light => "light",
            Look::HighContrast => "high_contrast",
        }
    }
    fn from_key(s: &str) -> Look {
        Look::ALL.into_iter().find(|l| l.key() == s).unwrap_or(Look::System)
    }
}

/// Semantic colour tokens for the app's own UI.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tokens {
    pub ok: Color32,
    pub warn: Color32,
    pub err: Color32,
    pub info: Color32,
    pub muted: Color32,
    /// Non-text accent (focus ring, primary outline); >= 3:1.
    pub accent: Color32,
    pub focus_width: f32,
    pub pill_ok: Color32,
    pub pill_warn: Color32,
    pub pill_err: Color32,
    pub on_pill: Color32,
    pub primary_fill: Color32,
    pub on_primary: Color32,
    pub primary_stroke: Color32,
    pub card_fill: Color32,
    pub card_stroke: Color32,
    pub bar_fill: Color32,
    pub log_fill: Color32,
    pub high_contrast: bool,
}

const DARK: Tokens = Tokens {
    ok: Color32::from_rgb(110, 210, 120),
    warn: Color32::from_rgb(240, 190, 70),
    err: Color32::from_rgb(255, 128, 118),
    info: Color32::from_rgb(125, 185, 255),
    muted: Color32::from_gray(170),
    accent: Color32::from_rgb(90, 170, 255),
    focus_width: 2.0,
    pill_ok: Color32::from_rgb(20, 110, 40),
    pill_warn: Color32::from_rgb(150, 80, 0),
    pill_err: Color32::from_rgb(160, 30, 30),
    on_pill: Color32::WHITE,
    primary_fill: Color32::from_rgb(10, 90, 180),
    on_primary: Color32::WHITE,
    primary_stroke: Color32::from_rgb(120, 185, 255),
    card_fill: Color32::from_gray(32),
    card_stroke: Color32::from_gray(58),
    bar_fill: Color32::from_gray(21),
    log_fill: Color32::from_gray(16),
    high_contrast: false,
};

const LIGHT: Tokens = Tokens {
    ok: Color32::from_rgb(18, 110, 36),
    warn: Color32::from_rgb(140, 80, 0),
    err: Color32::from_rgb(185, 20, 20),
    info: Color32::from_rgb(0, 85, 170),
    muted: Color32::from_gray(88),
    accent: Color32::from_rgb(0, 95, 190),
    primary_fill: Color32::from_rgb(0, 86, 172),
    on_primary: Color32::WHITE,
    primary_stroke: Color32::from_rgb(0, 58, 118),
    card_fill: Color32::WHITE,
    card_stroke: Color32::from_gray(205),
    bar_fill: Color32::from_gray(235),
    log_fill: Color32::from_gray(250),
    ..DARK
};

const HIGH_CONTRAST: Tokens = Tokens {
    ok: Color32::from_rgb(80, 255, 120),
    warn: Color32::from_rgb(255, 230, 0),
    err: Color32::from_rgb(255, 130, 130),
    info: Color32::from_rgb(110, 220, 255),
    muted: Color32::from_gray(225),
    accent: Color32::from_rgb(255, 255, 0),
    focus_width: 3.0,
    pill_ok: Color32::BLACK,
    pill_warn: Color32::BLACK,
    pill_err: Color32::BLACK,
    on_pill: Color32::from_rgb(255, 255, 0),
    primary_fill: Color32::BLACK,
    on_primary: Color32::from_rgb(255, 255, 0),
    primary_stroke: Color32::from_rgb(255, 255, 0),
    card_fill: Color32::BLACK,
    card_stroke: Color32::WHITE,
    bar_fill: Color32::BLACK,
    log_fill: Color32::BLACK,
    high_contrast: true,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Skin {
    Dark,
    Light,
    Hc,
}

fn tokens(s: Skin) -> Tokens {
    match s {
        Skin::Dark => DARK,
        Skin::Light => LIGHT,
        Skin::Hc => HIGH_CONTRAST,
    }
}

thread_local! {
    static NOW: Cell<Tokens> = const { Cell::new(DARK) };
}

/// Tokens for the theme showing this frame.
pub fn t() -> Tokens {
    NOW.with(|c| c.get())
}

fn visuals(s: Skin) -> Visuals {
    let g = Color32::from_gray;
    match s {
        // Dark / Light: stock egui visuals, only text / link / selection retuned.
        Skin::Dark => {
            let mut v = Visuals::dark();
            v.widgets.noninteractive.fg_stroke.color = g(226);
            v.widgets.noninteractive.weak_bg_fill = g(60);
            v.widgets.inactive.fg_stroke.color = g(236);
            v.hyperlink_color = DARK.info;
            v.warn_fg_color = DARK.warn;
            v.error_fg_color = DARK.err;
            v.selection.stroke.color = Color32::from_rgb(212, 232, 255);
            v
        }
        Skin::Light => {
            let mut v = Visuals::light();
            v.widgets.noninteractive.fg_stroke.color = g(20);
            v.widgets.noninteractive.weak_bg_fill = g(180);
            v.widgets.inactive.fg_stroke.color = g(15);
            v.widgets.hovered.fg_stroke.color = g(0);
            v.widgets.active.fg_stroke.color = g(0);
            v.hyperlink_color = LIGHT.info;
            v.warn_fg_color = LIGHT.warn;
            v.error_fg_color = LIGHT.err;
            v.selection.stroke.color = Color32::from_rgb(0, 58, 110);
            v
        }
        // High contrast: fully custom chrome - black, white text, yellow accent, no shadows.
        Skin::Hc => {
            let (w, y, b) = (Color32::WHITE, HIGH_CONTRAST.accent, Color32::BLACK);
            let mut v = Visuals::dark();
            v.panel_fill = b;
            v.window_fill = b;
            v.extreme_bg_color = b;
            v.faint_bg_color = g(24);
            v.code_bg_color = g(30);
            v.window_stroke = Stroke::new(2.0_f32, w);
            v.window_shadow = egui::epaint::Shadow::NONE;
            v.popup_shadow = egui::epaint::Shadow::NONE;
            let wv = &mut v.widgets;
            for x in [&mut wv.noninteractive, &mut wv.inactive] {
                x.bg_fill = b;
                x.weak_bg_fill = b;
                x.bg_stroke = Stroke::new(1.5_f32, w);
                x.fg_stroke = Stroke::new(1.0_f32, w);
            }
            wv.noninteractive.weak_bg_fill = g(190);
            for x in [&mut wv.hovered, &mut wv.open] {
                x.bg_fill = g(40);
                x.weak_bg_fill = g(40);
                x.bg_stroke = Stroke::new(2.0_f32, y);
                x.fg_stroke = Stroke::new(1.5_f32, y);
            }
            wv.active.bg_fill = g(60);
            wv.active.weak_bg_fill = g(60);
            wv.active.bg_stroke = Stroke::new(3.0_f32, y);
            wv.active.fg_stroke = Stroke::new(2.0_f32, y);
            v.selection.bg_fill = Color32::from_rgb(0, 70, 170);
            v.selection.stroke = Stroke::new(2.0_f32, y);
            v.hyperlink_color = HIGH_CONTRAST.info;
            v.warn_fg_color = HIGH_CONTRAST.warn;
            v.error_fg_color = HIGH_CONTRAST.err;
            v.text_cursor.stroke = Stroke::new(2.0_f32, y);
            v
        }
    }
}

fn style(s: Skin, mono: f32) -> egui::Style {
    let mut st = egui::Style { visuals: visuals(s), ..Default::default() };
    st.text_styles = [
        (TextStyle::Small, FontId::new(10.5, FontFamily::Proportional)),
        (TextStyle::Body, FontId::new(13.5, FontFamily::Proportional)),
        (TextStyle::Button, FontId::new(13.5, FontFamily::Proportional)),
        (TextStyle::Heading, FontId::new(20.0, FontFamily::Proportional)),
        (TextStyle::Monospace, FontId::new(13.0 * mono, FontFamily::Monospace)),
    ]
    .into();
    let sp = &mut st.spacing;
    sp.item_spacing = egui::vec2(8.0, 6.0);
    sp.button_padding = egui::vec2(10.0, 4.0);
    sp.window_margin = egui::Margin::same(14);
    sp.menu_margin = egui::Margin::same(6);
    sp.indent = 16.0;
    sp.interact_size.y = 24.0;
    let v = &mut st.visuals;
    for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = egui::CornerRadius::same(5);
    }
    v.window_corner_radius = egui::CornerRadius::same(9);
    v.menu_corner_radius = egui::CornerRadius::same(6);
    if s != Skin::Hc {
        v.widgets.noninteractive.bg_stroke.color = tokens(s).card_stroke;
    }
    st
}

// ---------------- preferences (theme + text size), persisted ----------------
pub const SCALE_STEPS: [f32; 10] = [0.75, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0];
pub const MONO_STEPS: [f32; 7] = [0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 2.0];

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Prefs {
    pub look: Look,
    pub scale: f32,
    pub mono: f32,
    /// OneDrive wizard: compact + technical detail. Default OFF (beginner-friendly).
    pub expert: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs { look: Look::System, scale: 1.0, mono: 1.0, expert: false }
    }
}

fn clamp(v: f64, lo: f32, hi: f32) -> f32 {
    let v = v as f32;
    if v.is_finite() {
        (v.clamp(lo, hi) * 100.0).round() / 100.0
    } else {
        1.0
    }
}

pub fn step(cur: f32, up: bool) -> f32 {
    if up {
        SCALE_STEPS.iter().copied().find(|s| *s > cur + 0.001).unwrap_or(3.0)
    } else {
        SCALE_STEPS.iter().rev().copied().find(|s| *s < cur - 0.001).unwrap_or(0.75)
    }
}

impl Prefs {
    /// Missing / unreadable / garbage file => defaults (never fails).
    pub fn load(path: &std::path::Path) -> Prefs {
        let d = Prefs::default();
        let Ok(raw) = std::fs::read_to_string(path) else { return d };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(raw.trim_start_matches('\u{feff}')) else { return d };
        Prefs {
            look: v["theme"].as_str().map(Look::from_key).unwrap_or(d.look),
            scale: v["scale"].as_f64().map(|x| clamp(x, 0.75, 3.0)).unwrap_or(1.0),
            mono: v["mono_scale"].as_f64().map(|x| clamp(x, 0.8, 2.0)).unwrap_or(1.0),
            expert: v["expert_mode"].as_bool().unwrap_or(false),
        }
    }
    /// Best effort; if the folder isn't writable the choice just lives in memory.
    pub fn save(&self, path: &std::path::Path) {
        if let Some(p) = path.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        let v = serde_json::json!({ "theme": self.look.key(), "scale": self.scale, "mono_scale": self.mono, "expert_mode": self.expert });
        let _ = std::fs::write(path, serde_json::to_string_pretty(&v).unwrap_or_default());
    }
}

/// Apply theme + zoom + mono size. Call on start and when prefs change (not every frame).
pub fn apply(ctx: &egui::Context, p: &Prefs) {
    let dark_skin = if p.look == Look::HighContrast { Skin::Hc } else { Skin::Dark };
    ctx.set_style_of(Theme::Dark, style(dark_skin, p.mono));
    ctx.set_style_of(Theme::Light, style(Skin::Light, p.mono));
    ctx.options_mut(|o| {
        o.fallback_theme = Theme::Dark; // System with no OS answer => Dark
        o.zoom_with_keyboard = false; // Ctrl +/-/0 handled by the app (stepped, persisted)
    });
    ctx.set_theme(match p.look {
        Look::System => ThemePreference::System,
        Look::Dark | Look::HighContrast => ThemePreference::Dark,
        Look::Light => ThemePreference::Light,
    });
    ctx.set_zoom_factor(p.scale);
}

/// Pick this frame's tokens from the theme actually showing.
pub fn begin_frame(ctx: &egui::Context, p: &Prefs) {
    let s = if p.look == Look::HighContrast {
        Skin::Hc
    } else if ctx.style().visuals.dark_mode {
        Skin::Dark
    } else {
        Skin::Light
    };
    NOW.with(|c| c.set(tokens(s)));
}

/// Segoe UI first (Windows UI font), then symbol/CJK fallbacks, then egui's bundled fonts.
pub fn install_fonts(ctx: &egui::Context) {
    let dir = std::path::PathBuf::from(std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into())).join("Fonts");
    let read = |f: &str| std::fs::read(dir.join(f)).ok();
    let mut fonts = egui::FontDefinitions::default();
    if let Some(b) = read("segoeui.ttf") {
        fonts.font_data.insert("w10t-ui".into(), std::sync::Arc::new(egui::FontData::from_owned(b)));
        fonts.families.entry(FontFamily::Proportional).or_default().insert(0, "w10t-ui".into());
    }
    for (i, f) in ["seguisym.ttf", "msyh.ttc", "YuGothM.ttc", "malgun.ttf"].iter().enumerate() {
        if let Some(b) = read(f) {
            let n = format!("w10t-fb{i}");
            fonts.font_data.insert(n.clone(), std::sync::Arc::new(egui::FontData::from_owned(b)));
            for fam in [FontFamily::Proportional, FontFamily::Monospace] {
                fonts.families.entry(fam).or_default().push(n.clone());
            }
        }
    }
    ctx.set_fonts(fonts);
}

/// Visible outline around the widget with keyboard focus.
pub fn focus_ring(ctx: &egui::Context) {
    let Some(id) = ctx.memory(|m| m.focused()) else { return };
    let Some(r) = ctx.read_response(id) else { return };
    if !r.interact_rect.is_positive() {
        return;
    }
    let k = t();
    ctx.layer_painter(r.layer_id).rect_stroke(r.interact_rect.expand(2.0), 3.0, Stroke::new(k.focus_width, k.accent), egui::StrokeKind::Outside);
}

// ---------------- building blocks ----------------
pub fn card<R>(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let k = t();
    egui::Frame::new()
        .fill(k.card_fill)
        .stroke(Stroke::new(if k.high_contrast { 1.5_f32 } else { 1.0_f32 }, k.card_stroke))
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::same(12))
        .outer_margin(egui::Margin::symmetric(0, 4))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if !title.is_empty() {
                ui.label(egui::RichText::new(title).strong().size(14.5));
                ui.add_space(2.0);
            }
            add(ui)
        })
        .inner
}

pub fn pill(ui: &mut egui::Ui, text: &str, fill: Color32, tooltip: &str) -> egui::Response {
    let k = t();
    let r = egui::Frame::new()
        .fill(fill)
        .stroke(if k.high_contrast { Stroke::new(1.5_f32, k.on_pill) } else { Stroke::NONE })
        .corner_radius(egui::CornerRadius::same(10))
        .inner_margin(egui::Margin::symmetric(8, 2))
        .show(ui, |ui| ui.label(egui::RichText::new(text).color(k.on_pill).small().strong()))
        .response;
    if tooltip.is_empty() {
        r
    } else {
        r.on_hover_text(tooltip)
    }
}

/// Small "(i)" marker with a plain-language explanation on hover / focus.
pub fn info(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let k = t();
    ui.add(egui::Label::new(egui::RichText::new("(i)").strong().color(k.info)).sense(egui::Sense::hover())).on_hover_text(text)
}

/// Destructive / red-labelled button, same look as the existing Revert buttons.
pub fn danger_button(text: &str) -> egui::Button<'static> {
    let k = t();
    egui::Button::new(egui::RichText::new(text.to_string()).strong().color(k.err)).stroke(Stroke::new(1.5_f32, k.err)).min_size(egui::vec2(160.0, 30.0))
}

pub fn primary_button(text: &str) -> egui::Button<'static> {
    let k = t();
    egui::Button::new(egui::RichText::new(text.to_string()).strong().size(14.5).color(k.on_primary))
        .fill(k.primary_fill)
        .stroke(Stroke::new(1.5_f32, k.primary_stroke))
        .min_size(egui::vec2(180.0, 32.0))
}

/// WCAG 2 contrast ratio.
#[cfg_attr(not(test), allow(dead_code))]
pub fn contrast(a: Color32, b: Color32) -> f32 {
    fn lum(c: Color32) -> f32 {
        let ch = |v: u8| {
            let v = v as f32 / 255.0;
            if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * ch(c.r()) + 0.7152 * ch(c.g()) + 0.0722 * ch(c.b())
    }
    let (x, y) = (lum(a), lum(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn semantic_text_is_aa_on_its_backgrounds() {
        for s in [Skin::Dark, Skin::Light, Skin::Hc] {
            let k = tokens(s);
            let v = visuals(s);
            for bg in [v.panel_fill, v.window_fill, k.card_fill, k.bar_fill, k.log_fill] {
                for fg in [k.ok, k.warn, k.err, k.info, k.muted, v.text_color()] {
                    assert!(contrast(fg, bg) >= 4.5, "{s:?} {fg:?} on {bg:?} = {:.2}", contrast(fg, bg));
                }
            }
            for p in [k.pill_ok, k.pill_warn, k.pill_err] {
                assert!(contrast(k.on_pill, p) >= 4.5, "{s:?} pill");
            }
            assert!(contrast(k.on_primary, k.primary_fill) >= 4.5, "{s:?} primary");
            assert!(contrast(k.accent, v.panel_fill) >= 3.0 && contrast(k.accent, k.card_fill) >= 3.0, "{s:?} accent");
        }
    }
}
