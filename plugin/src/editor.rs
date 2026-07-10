//! The 8-bit editor: clean, playful, NES-era clarity. Built with nih_plug_egui.
//!
//! GUI is read-only on params via ParamSetter (begin/set/end). No audio-thread
//! work. The theme is installed once in the build closure.

use nih_plug::prelude::*;
use nih_plug_egui::{create_egui_editor, egui};
use std::sync::Arc;

use crate::params::{self, DrumgenParams};

// ── Sweetie-16 derived palette ──
const BG: egui::Color32 = egui::Color32::from_rgb(0x1A, 0x1C, 0x2C);
const PANEL: egui::Color32 = egui::Color32::from_rgb(0x33, 0x3C, 0x57);
const ACCENT_A: egui::Color32 = egui::Color32::from_rgb(0xA7, 0xF0, 0x70); // lime
const ACCENT_B: egui::Color32 = egui::Color32::from_rgb(0xFF, 0xCD, 0x75); // butter
const TEXT: egui::Color32 = egui::Color32::from_rgb(0xF4, 0xF4, 0xF4);
const DIM: egui::Color32 = egui::Color32::from_rgb(0x94, 0xB0, 0xC2);

fn install_theme(ctx: &egui::Context) {
    // Pixel font, installed once.
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "pixel".to_owned(),
        Arc::new(egui::FontData::from_static(include_bytes!("../assets/PressStart2P-Regular.ttf"))),
    );
    for fam in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts.families.entry(fam).or_default().insert(0, "pixel".to_owned());
    }
    ctx.set_fonts(fonts);

    use egui::{FontFamily, FontId, TextStyle};
    let mut style = (*ctx.style()).clone();
    style.text_styles = [
        (TextStyle::Heading, FontId::new(16.0, FontFamily::Proportional)),
        (TextStyle::Body, FontId::new(8.0, FontFamily::Proportional)),
        (TextStyle::Button, FontId::new(8.0, FontFamily::Proportional)),
        (TextStyle::Small, FontId::new(8.0, FontFamily::Proportional)),
        (TextStyle::Monospace, FontId::new(8.0, FontFamily::Monospace)),
    ]
    .into();
    style.interaction.selectable_labels = false;

    // 8-bit visuals: hard corners, 2px borders, flat fills, no shadows.
    let corner = egui::CornerRadius::ZERO;
    let mut v = egui::Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = BG;
    v.extreme_bg_color = BG;
    v.faint_bg_color = PANEL;
    v.window_corner_radius = corner;
    v.menu_corner_radius = corner;
    v.window_shadow = egui::epaint::Shadow::NONE;
    v.popup_shadow = egui::epaint::Shadow::NONE;
    v.selection.bg_fill = ACCENT_A;
    v.selection.stroke = egui::Stroke::new(2.0f32, BG);
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = corner;
        w.expansion = 0.0;
    }
    v.widgets.noninteractive.bg_stroke = egui::Stroke::new(2.0f32, PANEL);
    v.widgets.noninteractive.fg_stroke = egui::Stroke::new(2.0f32, TEXT);
    v.widgets.inactive.bg_fill = PANEL;
    v.widgets.inactive.weak_bg_fill = PANEL;
    v.widgets.inactive.bg_stroke = egui::Stroke::new(2.0f32, TEXT);
    v.widgets.inactive.fg_stroke = egui::Stroke::new(2.0f32, TEXT);
    v.widgets.hovered.bg_fill = PANEL;
    v.widgets.hovered.bg_stroke = egui::Stroke::new(2.0f32, ACCENT_B);
    v.widgets.hovered.fg_stroke = egui::Stroke::new(2.0f32, TEXT);
    v.widgets.active.bg_fill = ACCENT_A;
    v.widgets.active.bg_stroke = egui::Stroke::new(2.0f32, ACCENT_A);
    v.widgets.active.fg_stroke = egui::Stroke::new(2.0f32, BG);
    style.visuals = v;

    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(8.0, 6.0);
    style.spacing.interact_size = egui::vec2(24.0, 24.0);
    ctx.set_style(style);

    ctx.options_mut(|o| o.tessellation_options.feathering = false);
}

/// Nudge an IntParam by `delta`, wrapping in `[0, count)`.
fn step_int(setter: &ParamSetter, p: &IntParam, cur: i32, delta: i32, count: i32) {
    let next = (cur + delta).rem_euclid(count.max(1));
    setter.begin_set_parameter(p);
    setter.set_parameter(p, next);
    setter.end_set_parameter(p);
}

/// A labelled ◀ value ▶ stepper. Returns the chosen delta (-1/0/+1).
fn stepper(ui: &mut egui::Ui, label: &str, value: &str) -> i32 {
    let mut delta = 0;
    ui.vertical(|ui| {
        ui.label(egui::RichText::new(label).color(DIM));
        ui.horizontal(|ui| {
            if ui.button("◀").clicked() {
                delta = -1;
            }
            ui.add_sized([96.0, 24.0], egui::Label::new(egui::RichText::new(value).color(TEXT)));
            if ui.button("▶").clicked() {
                delta = 1;
            }
        });
    });
    delta
}

/// A vertical pixel "knob": drag to change a 0..1 FloatParam. Draws a stack of
/// square cells that light up to the current value.
fn pixel_knob(ui: &mut egui::Ui, setter: &ParamSetter, label: &str, p: &FloatParam) {
    ui.vertical(|ui| {
        ui.label(egui::RichText::new(label).color(DIM));
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(48.0, 56.0), egui::Sense::drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, PANEL);
        painter.rect_stroke(rect, 0.0, egui::Stroke::new(2.0f32, TEXT), egui::StrokeKind::Inside);

        let value = p.value().clamp(0.0, 1.0);
        let cells = 7;
        let lit = (value * cells as f32).round() as i32;
        let inner = rect.shrink(6.0);
        let ch = inner.height() / cells as f32;
        for i in 0..cells {
            let from_bottom = cells - 1 - i;
            let y = inner.top() + i as f32 * ch;
            let cell = egui::Rect::from_min_size(
                egui::pos2(inner.left(), y + 1.0),
                egui::vec2(inner.width(), ch - 2.0),
            );
            let on = from_bottom < lit;
            painter.rect_filled(cell, 0.0, if on { ACCENT_A } else { BG });
        }

        if resp.dragged() {
            let dv = -resp.drag_delta().y / 100.0;
            let nv = (value + dv).clamp(0.0, 1.0);
            setter.begin_set_parameter(p);
            setter.set_parameter(p, nv);
            setter.end_set_parameter(p);
        } else if resp.drag_started() {
            setter.begin_set_parameter(p);
        }

        ui.label(egui::RichText::new(format!("{:>3.0}%", value * 100.0)).color(TEXT));
    });
}

pub fn create(params: Arc<DrumgenParams>, n_styles: usize) -> Option<Box<dyn Editor>> {
    let egui_state = params.editor_state.clone();
    create_egui_editor(
        egui_state,
        (),
        |ctx, _| install_theme(ctx),
        move |ctx, setter, _| {
            egui::CentralPanel::default()
                .frame(egui::Frame::default().fill(BG).inner_margin(8.0))
                .show(ctx, |ui| {
                    // Header: logo + style picker.
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("DRUMGEN").heading().color(ACCENT_A));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("▶").clicked() {
                                step_int(setter, &params.style, params.style.value(), 1, n_styles as i32);
                            }
                            ui.add_sized(
                                [180.0, 24.0],
                                egui::Label::new(egui::RichText::new(params.style.to_string()).color(TEXT)),
                            );
                            if ui.button("◀").clicked() {
                                step_int(setter, &params.style, params.style.value(), -1, n_styles as i32);
                            }
                        });
                    });
                    ui.separator();

                    // Control row: knobs + steppers.
                    ui.horizontal_top(|ui| {
                        pixel_knob(ui, setter, "HUMANIZE", &params.humanize);
                        pixel_knob(ui, setter, "SWING", &params.swing);
                        ui.add_space(8.0);

                        let db = stepper(ui, "BARS", &params.bars.value().to_string());
                        if db != 0 {
                            let next = (params.bars.value() + db).clamp(1, 16);
                            setter.begin_set_parameter(&params.bars);
                            setter.set_parameter(&params.bars, next);
                            setter.end_set_parameter(&params.bars);
                        }

                        let dm = stepper(ui, "METER", &params.meter.to_string());
                        if dm != 0 {
                            step_int(setter, &params.meter, params.meter.value(), dm, params::METERS.len() as i32);
                        }
                    });

                    ui.add_space(8.0);

                    // DICE hero + SAVE stub.
                    ui.horizontal(|ui| {
                        let dice = ui.add_sized(
                            [96.0, 40.0],
                            egui::Button::new(egui::RichText::new("⚄ DICE").color(BG)).fill(ACCENT_A),
                        );
                        if dice.clicked() {
                            let next = (params.seed.value() + 1) % 10000;
                            setter.begin_set_parameter(&params.seed);
                            setter.set_parameter(&params.seed, next);
                            setter.end_set_parameter(&params.seed);
                        }
                        ui.add_enabled(false, egui::Button::new("SAVE .MID"))
                            .on_disabled_hover_text("coming soon");
                    });

                    // Status line.
                    ui.add_space(8.0);
                    let status = format!(
                        "SEED {:04}  {}  {}  {} BARS",
                        params.seed.value(),
                        params.style.to_string(),
                        params.meter.to_string(),
                        params.bars.value(),
                    );
                    ui.label(egui::RichText::new(status).color(DIM));
                });
        },
    )
}
