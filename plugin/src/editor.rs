//! The 8-bit editor: clean, playful, NES-era clarity. Built with nih_plug_egui.
//!
//! GUI is read-only on params via ParamSetter (begin/set/end). No audio-thread
//! work. The theme is installed once in the build closure.

use nih_plug::prelude::*;
use nih_plug_egui::{create_egui_editor, egui};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use crate::engine::midi_math::PPQ;
use crate::export;
use crate::params::{self, DrumgenParams};
use crate::pattern::Pattern;

// ── Sweetie-16 derived palette ──
const BG: egui::Color32 = egui::Color32::from_rgb(0x1A, 0x1C, 0x2C);
const PANEL: egui::Color32 = egui::Color32::from_rgb(0x33, 0x3C, 0x57);
const ACCENT_A: egui::Color32 = egui::Color32::from_rgb(0xA7, 0xF0, 0x70); // lime
const ACCENT_B: egui::Color32 = egui::Color32::from_rgb(0xFF, 0xCD, 0x75); // butter
const TEXT: egui::Color32 = egui::Color32::from_rgb(0xF4, 0xF4, 0xF4);
const DIM: egui::Color32 = egui::Color32::from_rgb(0x94, 0xB0, 0xC2);
// Step-grid velocity shades (lime ramp derived from ACCENT_A).
const GRID_MID: egui::Color32 = egui::Color32::from_rgb(0x6E, 0xA0, 0x4B);
const GRID_FAINT: egui::Color32 = egui::Color32::from_rgb(0x3E, 0x5C, 0x38);

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
fn stepper(ui: &mut egui::Ui, label: &str, value: &str, value_width: f32) -> i32 {
    let mut delta = 0;
    ui.vertical(|ui| {
        ui.label(egui::RichText::new(label).color(DIM));
        ui.horizontal(|ui| {
            if ui.button("◀").clicked() {
                delta = -1;
            }
            ui.add_sized(
                [value_width, 24.0],
                egui::Label::new(egui::RichText::new(value).color(TEXT)),
            );
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
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(40.0, 40.0), egui::Sense::drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, PANEL);
        painter.rect_stroke(rect, 0.0, egui::Stroke::new(2.0f32, TEXT), egui::StrokeKind::Inside);

        let value = p.value().clamp(0.0, 1.0);
        let cells = 5;
        let lit = (value * cells as f32).round() as i32;
        let inner = rect.shrink(4.0);
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
        resp.on_hover_text("drag up/down");

        ui.label(egui::RichText::new(format!("{:>3.0}%", value * 100.0)).color(TEXT));
    });
}

/// Draggable seed readout: horizontal/vertical drag scrubs the SEED param.
/// `acc` carries sub-pixel drag remainder across frames (slow drags register).
fn seed_drag(ui: &mut egui::Ui, setter: &ParamSetter, p: &IntParam, acc: &mut f32) {
    ui.vertical(|ui| {
        ui.label(egui::RichText::new("SEED").color(DIM));
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(64.0, 24.0), egui::Sense::drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, PANEL);
        painter.rect_stroke(rect, 0.0, egui::Stroke::new(2.0f32, TEXT), egui::StrokeKind::Inside);
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            format!("{:04}", p.value()),
            egui::FontId::proportional(8.0),
            TEXT,
        );
        if resp.dragged() {
            *acc += resp.drag_delta().x - resp.drag_delta().y;
            let steps = *acc as i32;
            if steps != 0 {
                *acc -= steps as f32;
                let next = (p.value() + steps).rem_euclid(10000);
                setter.begin_set_parameter(p);
                setter.set_parameter(p, next);
                setter.end_set_parameter(p);
            }
        } else {
            *acc = 0.0;
        }
        resp.on_hover_text("drag to scrub seed");
    });
}

// ── Step-grid pattern preview ──

const GRID_LANES: usize = 6;
const LANE_LABELS: [&str; GRID_LANES] = ["CRA", "RID", "HAT", "TOM", "SNR", "KCK"];
const MAX_COLS: usize = 24; // covers every shipped meter (6/4 = 24 sixteenths)

/// GM drum note → grid lane (top to bottom: crash, ride, hats, toms, snare, kick).
fn lane_of(note: u8) -> Option<usize> {
    match note {
        49 | 50 | 57 | 58 | 52 | 61 | 55 | 56 | 59 => Some(0), // crashes/china/splash/fx
        51 | 53 | 54 => Some(1),                               // ride family
        42 | 46 | 32 | 44 => Some(2),                          // hihats
        48 | 47 | 45 | 43 | 41 => Some(3),                     // toms
        38 | 37 => Some(4),                                    // snare
        36 => Some(5),                                         // kick
        _ => None,
    }
}

/// Bar-1 sixteenth grid: max note-on velocity per (lane, column), plus the
/// number of sixteenth columns bar 1 actually has (capped at MAX_COLS).
fn grid_cells(pattern: &Pattern, bar: usize) -> ([[u8; MAX_COLS]; GRID_LANES], usize) {
    let mut grid = [[0u8; MAX_COLS]; GRID_LANES];
    let start = pattern.bar_starts.get(bar).copied().unwrap_or(0);
    let end = pattern
        .bar_starts
        .get(bar + 1)
        .copied()
        .unwrap_or(pattern.total_ticks);
    let sixteenth = PPQ / 4;
    let len = (end - start).max(1);
    let ncols = ((len + sixteenth - 1) / sixteenth).clamp(1, MAX_COLS as i64) as usize;
    for ev in &pattern.events {
        if !ev.is_note_on || ev.tick < start || ev.tick >= end {
            continue;
        }
        if let Some(lane) = lane_of(ev.note) {
            // Round to the nearest sixteenth so humanized jitter stays on-cell.
            let col = ((ev.tick - start + sixteenth / 2) / sixteenth) as usize;
            let col = col.min(ncols - 1);
            grid[lane][col] = grid[lane][col].max(ev.velocity);
        }
    }
    (grid, ncols)
}

/// Render the bar-1 step grid with a "what am I hearing" header above it.
/// Everything shown — style, meter, bars, seed — comes from the SAME pattern
/// snapshot, so the labels always match the notes (no param-vs-snapshot race).
/// What the drummer does next: (line text, hot). `ph_bar` is the 0-based
/// playhead bar (-1 = stopped -> None). Song mode walks the section map;
/// loop mode counts down to the next fill bar. Pure — unit tested.
fn telegraph(
    sections: &[(String, i32)],
    total_bars: usize,
    ph_bar: i64,
    fill_every: i32,
) -> Option<(String, bool)> {
    if ph_bar < 0 || total_bars == 0 {
        return None;
    }
    let bar = (ph_bar as usize % total_bars) as i32 + 1; // 1-based

    if !sections.is_empty() {
        // Song mode: current section + what's next (wrapping to the top).
        let mut cum = 0;
        for (i, (name, bars)) in sections.iter().enumerate() {
            let start = cum + 1;
            cum += bars;
            if bar <= cum {
                let (next_name, _) = &sections[(i + 1) % sections.len()];
                let left = cum - bar + 1; // bars remaining incl. current
                let hot = bar == start; // just landed on this section
                let line = if hot {
                    format!("▸ {} NOW", name.to_uppercase())
                } else {
                    format!("{} ▸ {} IN {}", name.to_uppercase(), next_name.to_uppercase(), left)
                };
                return Some((line, hot || left == 1));
            }
        }
        return None;
    }

    if fill_every > 0 {
        // Loop mode: count down to the next fill bar (bar % fill_every == 0).
        if bar % fill_every == 0 {
            return Some(("▸ FILL NOW".to_string(), true));
        }
        let left = fill_every - (bar % fill_every);
        return Some((format!("FILL IN {}", left), left == 1));
    }
    None
}

fn step_grid(ui: &mut egui::Ui, pattern: &Pattern, view_bar: &mut usize, ph_bar: i64, follow: &mut u8) {
    let total_bars = pattern.bar_starts.len().saturating_sub(1).max(1);
    // Clamp against the current pattern (BARS may have shrunk since last frame).
    if *view_bar >= total_bars {
        *view_bar = 0;
    }
    // Follow the playhead while the transport runs: NOW shows the sounding
    // bar, NEXT (jam default) shows the bar the drummer plays next, wrapped.
    let playing = ph_bar >= 0;
    if playing && *follow > 0 {
        let offset = if *follow == 2 { 1 } else { 0 };
        *view_bar = (ph_bar as usize + offset) % total_bars;
    }
    let (grid, ncols) = grid_cells(pattern, *view_bar);
    // Meter of the VIEWED bar — song sections change meter mid-pattern.
    let bar_number = (*view_bar + 1) as i32;
    let (num, den) = pattern
        .time_signatures
        .iter()
        .find(|ts| ts.bar_start <= bar_number && bar_number <= ts.bar_end)
        .or(pattern.time_signatures.first())
        .map(|ts| (ts.numerator, ts.denominator))
        .unwrap_or((4, 4));
    // Section name of the viewed bar (song mode only; empty in loop mode).
    let section = {
        let mut cum = 0;
        pattern
            .sections
            .iter()
            .find(|(_, bars)| {
                cum += bars;
                bar_number <= cum
            })
            .map(|(name, _)| name.to_uppercase())
    };

    // Header states the *generated truth*: style + the VIEWED bar's meter and
    // section. BAR n/N is a pager — click to see the next bar.
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("{} {}/{}", pattern.style_name.to_uppercase(), num, den))
                .color(ACCENT_B),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let label = match (playing, *follow) {
                (true, 1) => format!("NOW {}/{}", *view_bar + 1, total_bars),
                (true, 2) => format!("NEXT {}/{}", *view_bar + 1, total_bars),
                _ => format!("BAR {}/{}", *view_bar + 1, total_bars),
            };
            let pager = ui.button(label).on_hover_text(if playing {
                "click: cycle BAR/NOW/NEXT follow"
            } else {
                "click: view next bar"
            });
            if pager.clicked() {
                if playing {
                    *follow = (*follow + 1) % 3;
                } else {
                    *view_bar = (*view_bar + 1) % total_bars;
                }
            }
            if let Some(sec) = section {
                ui.label(egui::RichText::new(sec).color(DIM));
            }
        });
    });

    // Beat-pulse columns: one beat = 4 sixteenths in /4 meters, 2 in /8.
    let pulse = if den == 8 { 2 } else { 4 };
    let gutter = 30.0;
    let avail = ui.available_width();
    let cell_w = ((avail - gutter) / ncols as f32).floor().clamp(6.0, 27.0);
    let cell_h = 13.0;
    let w = gutter + cell_w * ncols as f32;
    let h = cell_h * GRID_LANES as f32;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    for lane in 0..GRID_LANES {
        let y = rect.top() + lane as f32 * cell_h;
        painter.text(
            egui::pos2(rect.left(), y + cell_h * 0.5),
            egui::Align2::LEFT_CENTER,
            LANE_LABELS[lane],
            egui::FontId::proportional(8.0),
            DIM,
        );
        for col in 0..ncols {
            let x = rect.left() + gutter + col as f32 * cell_w;
            let cell = egui::Rect::from_min_size(
                egui::pos2(x + 1.0, y + 1.0),
                egui::vec2(cell_w - 2.0, cell_h - 2.0),
            );
            let v = grid[lane][col];
            let color = if v >= 96 {
                ACCENT_A
            } else if v >= 56 {
                GRID_MID
            } else if v >= 1 {
                GRID_FAINT
            } else if col % pulse == 0 {
                // Empty beat column — mark it so the pulse is readable.
                PANEL
            } else {
                BG
            };
            painter.rect_filled(cell, 0.0, color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::midi_math::TimeSigEntry;
    use crate::pattern::MidiEvent;

    fn test_pattern(events: Vec<MidiEvent>) -> Pattern {
        Pattern {
            events,
            total_ticks: 2 * 4 * PPQ,
            bar_starts: vec![0, 4 * PPQ, 8 * PPQ],
            time_signatures: vec![TimeSigEntry { bar_start: 1, bar_end: 2, numerator: 4, denominator: 4 }],
            generation: 0,
            seed: 0,
            tempo: 120.0,
            style_name: "test".into(),
            cell_name: String::new(),
            sections: Vec::new(),
        }
    }

    fn on(tick: i64, note: u8, velocity: u8) -> MidiEvent {
        MidiEvent { tick, note, velocity, is_note_on: true }
    }

    #[test]
    fn grid_cells_maps_bar1_hits() {
        let sixteenth = PPQ / 4;
        let pat = test_pattern(vec![
            on(0, 36, 110),                      // kick, beat 1 → lane 5 col 0, accent
            on(2 * PPQ + 7, 38, 70),             // snare beat 3, jittered late → lane 4 col 8
            on(15 * sixteenth - 5, 42, 40),      // hihat last 16th, jittered early → lane 2 col 15
            on(5 * PPQ, 49, 127),                // crash in bar 2 → excluded from bar-1 view
            MidiEvent { tick: 0, note: 36, velocity: 0, is_note_on: false }, // note-off ignored
        ]);
        let (grid, ncols) = grid_cells(&pat, 0);
        assert_eq!(ncols, 16);
        assert_eq!(grid[5][0], 110);
        assert_eq!(grid[4][8], 70);
        assert_eq!(grid[2][15], 40);
        assert!(grid[0].iter().all(|&v| v == 0), "bar-2 crash must not leak into bar 1");

        // Pager: bar 2's window must show that crash (beat 2 -> col 4).
        let (grid2, ncols2) = grid_cells(&pat, 1);
        assert_eq!(ncols2, 16);
        assert_eq!(grid2[0][4], 127, "bar-2 view must contain the bar-2 crash");
    }

    #[test]
    fn telegraph_loop_mode_counts_down_to_fill() {
        let none: &[(String, i32)] = &[];
        assert_eq!(
            telegraph(none, 4, 0, 4),
            Some(("FILL IN 3".to_string(), false))
        );
        assert_eq!(telegraph(none, 4, 2, 4), Some(("FILL IN 1".to_string(), true)));
        assert_eq!(telegraph(none, 4, 3, 4), Some(("▸ FILL NOW".to_string(), true)));
        // FILL off, no sections: nothing to telegraph.
        assert_eq!(telegraph(none, 4, 1, 0), None);
    }

    #[test]
    fn telegraph_song_mode_walks_sections() {
        let sections = vec![
            ("intro".to_string(), 2),
            ("build".to_string(), 2),
            ("blast".to_string(), 4),
        ];
        assert_eq!(
            telegraph(&sections, 8, 0, 0),
            Some(("▸ INTRO NOW".to_string(), true))
        );
        assert_eq!(
            telegraph(&sections, 8, 1, 0),
            Some(("INTRO ▸ BUILD IN 1".to_string(), true))
        );
        assert_eq!(
            telegraph(&sections, 8, 4, 0),
            Some(("▸ BLAST NOW".to_string(), true))
        );
        // Mid-blast: next wraps to the top of the form.
        assert_eq!(
            telegraph(&sections, 8, 5, 0),
            Some(("BLAST ▸ INTRO IN 3".to_string(), false))
        );
    }

    #[test]
    fn telegraph_wraps_playhead_past_pattern_end() {
        let sections = vec![("intro".to_string(), 2), ("blast".to_string(), 2)];
        // ph 5 wraps to bar 2 (0-based 1) -> intro's last bar.
        assert_eq!(telegraph(&sections, 4, 5, 0), telegraph(&sections, 4, 1, 0));
    }

    #[test]
    fn telegraph_stopped_is_silent() {
        let sections = vec![("intro".to_string(), 2)];
        assert_eq!(telegraph(&sections, 2, -1, 4), None);
        let none: &[(String, i32)] = &[];
        assert_eq!(telegraph(none, 0, 0, 4), None);
    }

    #[test]
    fn lane_of_covers_every_instrument_note() {
        // lane_of hardcodes note numbers; this pins it to the single source of
        // truth (Instrument::midi_note) so a kit remap can't silently drop an
        // instrument from the preview grid.
        use crate::engine::cell::Instrument;
        for inst in Instrument::ALL {
            assert!(
                lane_of(inst.midi_note()).is_some(),
                "{:?} (note {}) has no grid lane",
                inst,
                inst.midi_note()
            );
        }
    }

    #[test]
    fn grid_cells_odd_meter_column_count() {
        let mut pat = test_pattern(vec![on(0, 36, 100)]);
        // One 7/8 bar = 7 eighths = 14 sixteenths = 7 * PPQ/2 ticks.
        pat.bar_starts = vec![0, 7 * PPQ / 2, 7 * PPQ];
        pat.time_signatures =
            vec![TimeSigEntry { bar_start: 1, bar_end: 2, numerator: 7, denominator: 8 }];
        let (_, ncols) = grid_cells(&pat, 0);
        assert_eq!(ncols, 14);

        // 5/4 = 20 sixteenths and 6/4 = 24 must fit without truncation.
        let mut pat = test_pattern(vec![on(0, 36, 100)]);
        pat.bar_starts = vec![0, 5 * PPQ, 10 * PPQ];
        pat.time_signatures =
            vec![TimeSigEntry { bar_start: 1, bar_end: 2, numerator: 5, denominator: 4 }];
        let (_, ncols) = grid_cells(&pat, 0);
        assert_eq!(ncols, 20);

        let mut pat = test_pattern(vec![on(0, 36, 100)]);
        pat.bar_starts = vec![0, 6 * PPQ, 12 * PPQ];
        pat.time_signatures =
            vec![TimeSigEntry { bar_start: 1, bar_end: 2, numerator: 6, denominator: 4 }];
        let (_, ncols) = grid_cells(&pat, 0);
        assert_eq!(ncols, 24);
    }

    #[test]
    fn theme_and_font_bake_without_panic() {
        // Exercise the risky runtime path headlessly: embedding the pixel font
        // and baking the glyph atlas + theme. A corrupt TTF or misused egui API
        // would panic here instead of only in the host.
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            install_theme(ctx);
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.heading("DRUMGEN");
                ui.label("posthardcore");
            });
        });
    }
}

/// Editor-local UI state (not persisted).
#[derive(Default)]
struct UiState {
    /// Last SAVE .MID outcome.
    save_msg: String,
    /// Sub-pixel remainder for the SEED drag widget.
    seed_acc: f32,
    /// Which bar the step grid shows (0-based; clamped to the pattern).
    view_bar: usize,
    /// Grid follow mode while the transport runs: 0 = MANUAL (pager), 1 = NOW
    /// (playhead bar), 2 = NEXT (upcoming bar — the jam default).
    follow: u8,
}

impl UiState {
    fn new() -> Self {
        Self { follow: 2, ..Self::default() }
    }
}

pub fn create(
    params: Arc<DrumgenParams>,
    n_styles: usize,
    pattern_view: Arc<Mutex<Arc<Pattern>>>,
    playhead_bar: Arc<AtomicI64>,
) -> Option<Box<dyn Editor>> {
    let egui_state = params.editor_state.clone();
    create_egui_editor(
        egui_state,
        UiState::new(),
        |ctx, _| install_theme(ctx),
        move |ctx, setter, ui_state| {
            // Patterns arrive asynchronously from the worker; poll so the grid
            // refreshes without needing mouse movement.
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
            let pattern = pattern_view.lock().unwrap_or_else(|e| e.into_inner()).clone();

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
                        ui.add_space(4.0);

                        // In song mode the arrangement string owns length and
                        // fills; BARS and FILL go dead. METER stays live as
                        // the song's home meter.
                        let song_off = params::song_str(params.song.value()).is_empty();
                        ui.add_enabled_ui(song_off, |ui| {
                            let db = stepper(ui, "BARS", &params.bars.value().to_string(), 32.0);
                            if db != 0 {
                                let next = (params.bars.value() + db).clamp(1, 16);
                                setter.begin_set_parameter(&params.bars);
                                setter.set_parameter(&params.bars, next);
                                setter.end_set_parameter(&params.bars);
                            }
                        });

                        let dm = stepper(ui, "METER", &params.meter.to_string(), 40.0);
                        if dm != 0 {
                            step_int(setter, &params.meter, params.meter.value(), dm, params::METERS.len() as i32);
                        }

                        ui.add_enabled_ui(song_off, |ui| {
                            let df = stepper(ui, "FILL", &params.fill.to_string(), 56.0);
                            if df != 0 {
                                step_int(setter, &params.fill, params.fill.value(), df, params::FILLS.len() as i32);
                            }
                        });
                    });

                    ui.add_space(4.0);

                    // SONG structure + DICE hero + SAVE + last save result.
                    ui.horizontal(|ui| {
                        let ds = stepper(ui, "SONG", &params.song.to_string(), 80.0);
                        if ds != 0 {
                            step_int(setter, &params.song, params.song.value(), ds, params::n_songs() as i32);
                        }
                        let dice = ui
                            .add_sized(
                                [96.0, 40.0],
                                egui::Button::new(egui::RichText::new("⚄ DICE").color(BG)).fill(ACCENT_A),
                            )
                            .on_hover_text("new groove (seed +1)");
                        if dice.clicked() {
                            let next = (params.seed.value() + 1) % 10000;
                            setter.begin_set_parameter(&params.seed);
                            setter.set_parameter(&params.seed, next);
                            setter.end_set_parameter(&params.seed);
                        }
                        seed_drag(ui, setter, &params.seed, &mut ui_state.seed_acc);
                        let save = ui
                            .button("SAVE .MID")
                            .on_hover_text("write pattern to ~/drumgen_output");
                        if save.clicked() {
                            ui_state.save_msg = match export::save_pattern(&pattern) {
                                Ok(path) => format!(
                                    "SAVED {}",
                                    path.file_name().and_then(|n| n.to_str()).unwrap_or("?")
                                ),
                                Err(e) => format!("SAVE FAILED: {e}"),
                            };
                        }
                        if !ui_state.save_msg.is_empty() {
                            ui.label(egui::RichText::new(&ui_state.save_msg).color(DIM));
                        }
                    });

                    ui.add_space(4.0);

                    // Telegraph: what the drummer does next, readable mid-riff.
                    let ph_bar = playhead_bar.load(Ordering::Relaxed);
                    let total_bars = pattern.bar_starts.len().saturating_sub(1).max(1);
                    if let Some((line, hot)) = telegraph(
                        &pattern.sections,
                        total_bars,
                        ph_bar,
                        params::fill_of(params.fill.value()),
                    ) {
                        ui.label(
                            egui::RichText::new(line)
                                .color(if hot { ACCENT_A } else { ACCENT_B }),
                        );
                    }

                    // Step grid: follows the playhead while playing (NOW/NEXT),
                    // manual pager when stopped.
                    step_grid(ui, &pattern, &mut ui_state.view_bar, ph_bar, &mut ui_state.follow);
                });
        },
    )
}
