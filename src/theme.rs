//! Dark-only colour palette and egui styling, plus small shared widgets.

use eframe::egui::{self, Color32, CornerRadius, Margin, RichText, Stroke};

/// App background, behind images.
pub const BG: Color32 = Color32::from_rgb(0x0F, 0x11, 0x14);
/// Top/bottom bars and the setup screen.
pub const PANEL: Color32 = Color32::from_rgb(0x16, 0x19, 0x1D);
/// Cards, inputs and buttons.
pub const SURFACE: Color32 = Color32::from_rgb(0x1E, 0x23, 0x2A);
pub const SURFACE_HI: Color32 = Color32::from_rgb(0x2A, 0x30, 0x38);
pub const BORDER: Color32 = Color32::from_rgb(0x2C, 0x33, 0x3C);
pub const TEXT: Color32 = Color32::from_rgb(0xE6, 0xE9, 0xED);
pub const MUTED: Color32 = Color32::from_rgb(0x8B, 0x95, 0xA1);
pub const ACCENT: Color32 = Color32::from_rgb(0x3D, 0xD6, 0xB5);
pub const ACCENT_DIM: Color32 = Color32::from_rgb(0x1F, 0x6B, 0x5B);
pub const WARN: Color32 = Color32::from_rgb(0xF2, 0xC1, 0x4E);
pub const DANGER: Color32 = Color32::from_rgb(0xF2, 0x70, 0x6B);
/// The delete target gets its own colour so it never looks like a class.
pub const DELETE: Color32 = Color32::from_rgb(0xE5, 0x5B, 0x5B);

pub fn apply(ctx: &egui::Context) {
    // Dark only: ignore the OS light/dark preference.
    ctx.set_theme(egui::ThemePreference::Dark);

    let mut v = egui::Visuals::dark();
    v.panel_fill = PANEL;
    v.window_fill = SURFACE;
    v.window_stroke = Stroke::new(1.0, BORDER);
    v.extreme_bg_color = BG;
    v.text_edit_bg_color = Some(BG);
    v.faint_bg_color = SURFACE;
    v.code_bg_color = SURFACE;
    v.hyperlink_color = ACCENT;
    v.warn_fg_color = WARN;
    v.error_fg_color = DANGER;
    v.weak_text_color = Some(MUTED);
    v.selection.bg_fill = ACCENT_DIM;
    v.selection.stroke = Stroke::new(1.0, ACCENT);

    let r = CornerRadius::same(6);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = PANEL;
    w.noninteractive.weak_bg_fill = PANEL;
    w.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    w.noninteractive.corner_radius = r;
    for (wv, fill, stroke) in [
        (&mut w.inactive, SURFACE, Stroke::new(1.0, BORDER)),
        (&mut w.hovered, SURFACE_HI, Stroke::new(1.0, ACCENT_DIM)),
        (&mut w.active, ACCENT_DIM, Stroke::new(1.0, ACCENT)),
        (&mut w.open, SURFACE_HI, Stroke::new(1.0, BORDER)),
    ] {
        wv.bg_fill = fill;
        wv.weak_bg_fill = fill;
        wv.bg_stroke = stroke;
        wv.fg_stroke = Stroke::new(1.0, TEXT);
        wv.corner_radius = r;
        wv.expansion = 0.0;
    }
    ctx.set_visuals_of(egui::Theme::Dark, v);

    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(10.0, 4.0);
        s.spacing.interact_size.y = 24.0;
        if let Some(f) = s.text_styles.get_mut(&egui::TextStyle::Heading) {
            f.size = 24.0;
        }
        if let Some(f) = s.text_styles.get_mut(&egui::TextStyle::Body) {
            f.size = 14.0;
        }
        if let Some(f) = s.text_styles.get_mut(&egui::TextStyle::Button) {
            f.size = 14.0;
        }
    });
}

/// A rounded "keycap" label, e.g. for target hotkeys.
pub fn key_chip(ui: &mut egui::Ui, text: &str, fill: Color32, fg: Color32) {
    egui::Frame::new()
        .fill(fill)
        .corner_radius(4)
        .inner_margin(Margin::symmetric(6, 1))
        .show(ui, |ui| {
            ui.label(RichText::new(text).monospace().strong().color(fg));
        });
}

/// A muted keycap followed by what it does, for the shortcut legend.
pub fn hint(ui: &mut egui::Ui, key: &str, what: &str) {
    key_chip(ui, key, SURFACE_HI, TEXT);
    ui.label(RichText::new(what).color(MUTED));
    ui.add_space(6.0);
}

/// Frame for the top and bottom bars of the sorting screen.
pub fn bar_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(PANEL)
        .inner_margin(Margin::symmetric(12, 8))
}

/// A filled card used to group content on the setup screen.
pub fn card<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(10)
        .inner_margin(Margin::same(16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// Section title inside a card.
pub fn section(ui: &mut egui::Ui, title: &str) {
    ui.label(RichText::new(title).size(15.0).strong().color(TEXT));
}

/// The big accent-coloured call-to-action button.
pub fn primary_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(text).strong().size(15.0).color(BG))
            .fill(ACCENT)
            .stroke(Stroke::NONE)
            .corner_radius(8)
            .min_size(egui::vec2(180.0, 36.0)),
    )
}

/// Colour for a target's key chip.
pub fn target_color(key: egui::Key) -> Color32 {
    if key == egui::Key::Delete {
        DELETE
    } else {
        ACCENT
    }
}
