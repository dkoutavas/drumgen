//! The 8-bit editor: clean, playful, NES-era clarity. Built with nih_plug_egui.
//!
//! GUI is read-only on params via ParamSetter (begin/set/end). No audio-thread
//! work. The theme is installed once in the build closure.

use nih_plug::prelude::*;
use nih_plug_egui::resizable_window::ResizableWindow;
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
/// `hint` is the hover text on the title and the value (what it is now, what
/// a click does); `disabled_hint` replaces it while the stepper is greyed
/// out, to say why.
fn stepper(
    ui: &mut egui::Ui,
    label: &str,
    value: &str,
    value_width: f32,
    hint: &str,
    disabled_hint: &str,
) -> i32 {
    let mut delta = 0;
    let off_hint = if disabled_hint.is_empty() { hint } else { disabled_hint };
    ui.vertical(|ui| {
        ui.label(egui::RichText::new(label).color(DIM))
            .on_hover_text(tip(hint))
            .on_disabled_hover_text(tip(off_hint));
        ui.horizontal(|ui| {
            if ui.button("◀").clicked() {
                delta = -1;
            }
            ui.add_sized(
                [value_width, 24.0],
                egui::Label::new(egui::RichText::new(value).color(TEXT)),
            )
            .on_hover_text(tip(hint))
            .on_disabled_hover_text(tip(off_hint));
            if ui.button("▶").clicked() {
                delta = 1;
            }
        });
    });
    delta
}

/// A vertical pixel "knob": drag to change a 0..1 FloatParam. Draws a stack of
/// square cells that light up to the current value.
fn pixel_knob(ui: &mut egui::Ui, setter: &ParamSetter, label: &str, p: &FloatParam, hint: &str) {
    ui.vertical(|ui| {
        ui.label(egui::RichText::new(label).color(DIM)).on_hover_text(tip(hint));
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

        // One host gesture per drag: begin on press, set while it moves, end on
        // release. Emitting begin/set/end every frame made each frame its own
        // undo/automation entry, and the old `drag_started` branch after
        // `dragged` was unreachable (egui reports both on the first frame).
        if resp.drag_started() {
            setter.begin_set_parameter(p);
        }
        if resp.dragged() {
            let dv = -resp.drag_delta().y / 100.0;
            if dv != 0.0 {
                setter.set_parameter(p, (value + dv).clamp(0.0, 1.0));
            }
        }
        if resp.drag_stopped() {
            setter.end_set_parameter(p);
        }
        resp.on_hover_text(tip(hint));

        ui.label(egui::RichText::new(format!("{:>3.0}%", value * 100.0)).color(TEXT));
    });
}

/// The FORM SEED box: click rolls a new form (the same prime-stride dice as
/// DICE), drag scrubs. One widget for both, so the form's dice sits next to
/// the shape it rolls and the row keeps its width.
fn form_seed_box(ui: &mut egui::Ui, setter: &ParamSetter, p: &IntParam, shape: &str, acc: &mut f32) {
    let hint = format!(
        "{shape} {:04}: this seed is the form. Click: roll a new one · drag: scrub. \
         The same shape and seed give the same form every time, in the CLI too",
        p.value()
    );
    let off = "FORM is Off. Pick a shape with FORM, then roll it here";
    ui.vertical(|ui| {
        ui.label(egui::RichText::new("FORM SEED").color(DIM))
            .on_hover_text(tip(&hint))
            .on_disabled_hover_text(tip(off));
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(64.0, 24.0), egui::Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        let live = ui.is_enabled();
        painter.rect_filled(rect, 0.0, if live { ACCENT_B } else { PANEL });
        painter.rect_stroke(rect, 0.0, egui::Stroke::new(2.0f32, TEXT), egui::StrokeKind::Inside);
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            format!("{:04}", p.value()),
            egui::FontId::proportional(8.0),
            if live { BG } else { DIM },
        );
        if resp.clicked() {
            setter.begin_set_parameter(p);
            setter.set_parameter(p, params::dice_roll(p.value()));
            setter.end_set_parameter(p);
        }
        if resp.drag_started() {
            setter.begin_set_parameter(p);
        }
        if resp.dragged() {
            *acc += resp.drag_delta().x - resp.drag_delta().y;
            let steps = *acc as i32;
            if steps != 0 {
                *acc -= steps as f32;
                setter.set_parameter(p, (p.value() + steps).rem_euclid(10000));
            }
        } else {
            *acc = 0.0;
        }
        if resp.drag_stopped() {
            setter.end_set_parameter(p);
        }
        resp.on_hover_text(tip(&hint)).on_disabled_hover_text(tip(off));
    });
}

/// Draggable seed readout: horizontal/vertical drag scrubs the SEED param.
/// `acc` carries sub-pixel drag remainder across frames (slow drags register).
fn seed_drag(ui: &mut egui::Ui, setter: &ParamSetter, p: &IntParam, acc: &mut f32) {
    let hint = format!(
        "seed {:04} names this take: same seed and settings, same notes. Drag to scrub",
        p.value()
    );
    ui.vertical(|ui| {
        ui.label(egui::RichText::new("SEED").color(DIM)).on_hover_text(tip(&hint));
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
        // One gesture per scrub, same as the knobs: a drag across 200 seeds is
        // one undo step, not 200.
        if resp.drag_started() {
            setter.begin_set_parameter(p);
        }
        if resp.dragged() {
            *acc += resp.drag_delta().x - resp.drag_delta().y;
            let steps = *acc as i32;
            if steps != 0 {
                *acc -= steps as f32;
                let next = (p.value() + steps).rem_euclid(10000);
                setter.set_parameter(p, next);
            }
        } else {
            *acc = 0.0;
        }
        if resp.drag_stopped() {
            setter.end_set_parameter(p);
        }
        resp.on_hover_text(tip(&hint));
    });
}

// ── Step-grid pattern preview ──

const GRID_LANES: usize = 6;
const LANE_LABELS: [&str; GRID_LANES] = ["CRA", "RID", "HAT", "TOM", "SNR", "KCK"];
// The widest bar the plugin can be in: 15/4 = 60 sixteenths. That is the cap
// `songs.txt` enforces (1..=15 beats) and the cap on a normalized host meter,
// so no reachable bar is clamped. It was 24, which clamped 7/4 (28) and made
// every hit past column 23 pile into the last cell at full velocity.
const MAX_COLS: usize = 64;

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
/// number of sixteenth columns the bar actually has (capped at MAX_COLS).
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
/// Reduce the audio thread's published playhead TICK into this pattern
/// snapshot's bar index. Re-reduction (mod total_ticks) guards the pending-
/// swap window where the audio thread's pattern length differs from the GUI
/// snapshot. -1 = stopped.
fn ph_bar_of(tick: i64, pattern: &Pattern) -> i64 {
    if tick < 0 {
        return -1;
    }
    let t = tick.rem_euclid(pattern.total_ticks.max(1));
    (pattern.bar_starts.partition_point(|&b| b <= t) as i64 - 1).max(0)
}

/// What the drummer does next: (line text, hot). `ph_bar` is the 0-based
/// playhead bar (-1 = stopped -> None). Song mode walks the section map;
/// loop mode counts down to the next fill bar. Pure — unit tested.
fn telegraph(
    sections: &[(String, i32, String)],
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
        for (i, (name, bars, _)) in sections.iter().enumerate() {
            let start = cum + 1;
            cum += bars;
            if bar <= cum {
                let (next_name, _, _) = &sections[(i + 1) % sections.len()];
                let left = cum - bar + 1; // bars remaining incl. current
                let hot = bar == start; // just landed on this section
                let line = if hot {
                    format!("▶ {} NOW", name.to_uppercase())
                } else {
                    format!("{} ▶ {} IN {}", name.to_uppercase(), next_name.to_uppercase(), left)
                };
                return Some((line, hot || left == 1));
            }
        }
        return None;
    }

    if fill_every > 0 {
        // Loop mode: count down to the next fill bar (bar % fill_every == 0).
        if bar % fill_every == 0 {
            return Some(("▶ FILL NOW".to_string(), true));
        }
        let left = fill_every - (bar % fill_every);
        return Some((format!("FILL IN {}", left), left == 1));
    }
    None
}

/// The bank's line: a queued slot outranks the section/fill countdown, because
/// it is the thing the player just asked for. Pure — unit tested.
fn telegraph_bank(queued: i32) -> Option<(String, bool)> {
    (queued >= 0).then(|| (format!("▶ SLOT {} NEXT BAR", queued + 1), true))
}

/// The 16-slot pattern bank: store the current sound, trigger it later.
///
/// Reads the audio thread's `active`/`queued` atomics for display and writes
/// `trigger` for a click. Storing locks the bank outright — this is the GUI
/// thread, and the audio side only ever `try_lock`s.
fn bank_row(
    ui: &mut egui::Ui,
    params: &DrumgenParams,
    armed: &mut bool,
    blink_on: bool,
) {
    let bank = &params.bank;
    let active = bank.active.load(Ordering::Relaxed);
    let queued = bank.queued.load(Ordering::Relaxed);
    // A pad is only playable once the audio thread holds its pattern; showing
    // every stored pad as ready would promise a trigger that cannot fire yet.
    let ready = bank.ready.load(Ordering::Relaxed);

    ui.horizontal(|ui| {
        let store = ui
            .add_sized(
                [56.0, 24.0],
                egui::Button::new(
                    egui::RichText::new("STORE").color(if *armed { BG } else { TEXT }),
                )
                .fill(if *armed { ACCENT_B } else { PANEL }),
            )
            .on_hover_text(tip(if *armed {
                "STORE is armed: click a pad to store what is playing into it. \
                 Click STORE again to cancel"
            } else {
                "arm, then click a pad to copy what is playing into it: \
                 the pad that is playing, or the knobs when none is. ? explains the bank"
            }));
        if store.clicked() {
            *armed = !*armed;
        }

        // One lock per frame for the masks and the pad summaries (GUI
        // thread; the audio side only ever try_locks).
        let (filled_mask, summaries): (u16, Vec<Option<String>>) = {
            let guard = bank.lock();
            let sums = (0..params::BANK_SLOTS).map(|i| guard.slot(i).map(slot_summary)).collect();
            (guard.filled_mask(), sums)
        };
        let mut store_into: Option<usize> = None;
        let mut clear: Option<usize> = None;

        for (i, holds) in summaries.iter().enumerate() {
            let filled = filled_mask & (1 << i) != 0;
            let is_ready = ready & (1 << i) != 0;
            let is_active = i as i32 == active;
            let is_queued = i as i32 == queued;
            let (rect, resp) = ui.allocate_exact_size(
                egui::vec2(30.0, 24.0),
                egui::Sense::click(),
            );
            let state = pad_state(filled, is_ready, is_active);
            let fill = pad_fill(state, is_queued && blink_on);
            ui.painter().rect_filled(rect, egui::CornerRadius::ZERO, fill);
            // The playing pad gets a hard border too: on a pixel panel a
            // shade-only difference is not a difference.
            if is_active || is_queued {
                ui.painter().rect_stroke(
                    rect,
                    egui::CornerRadius::ZERO,
                    egui::Stroke::new(2.0_f32, TEXT),
                    egui::StrokeKind::Inside,
                );
            }
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                format!("{}", i + 1),
                egui::FontId::new(8.0, egui::FontFamily::Proportional),
                if is_active || (is_queued && blink_on) {
                    BG
                } else if filled {
                    TEXT
                } else {
                    DIM
                },
            );

            let resp = resp.on_hover_text(tip(pad_hint(
                i + 1,
                state,
                is_queued,
                *armed,
                holds.as_deref(),
            )));
            if resp.clicked() {
                if *armed {
                    store_into = Some(i);
                } else if filled {
                    // slot+1, so 0 can mean "nothing pending".
                    bank.trigger.store(i as i32 + 1, Ordering::Relaxed);
                }
            }
            if resp.secondary_clicked() {
                clear = Some(i);
            }
        }

        if let Some(i) = store_into {
            let style_index = params.style.value();
            let knobs = || params::SlotSnapshot {
                style_name: bank
                    .styles
                    .get(style_index as usize)
                    .cloned()
                    .unwrap_or_default(),
                style_index,
                humanize: params.humanize.value(),
                bars: params.bars.value(),
                seed: params.seed.value(),
                swing: params.swing.value(),
                meter: params.meter.value(),
                fill: params.fill.value(),
                song: params.song.value(),
                song_name: params::song_name_for(params.song.value()),
                form: params.form.value(),
                form_name: params::form_key(params.form.value()).to_string(),
                form_seed: params.form_seed.value(),
            };
            // The pad that is playing if one is, else the knobs.
            let snapshot = {
                let guard = bank.lock();
                params::store_source(active, &guard.slots, knobs)
            };
            bank.lock().slots[i] = Some(snapshot);
            bank.bump();
            *armed = false;
        }
        if let Some(i) = clear {
            bank.lock().slots[i] = None;
            bank.bump();
        }
    });
}

// ── Help: hover texts, pad states, the ? overlay ──

/// Hover text with room between wrapped lines: Press Start 2P has no line
/// gap, so at 8px two wrapped lines touch.
fn tip(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text.into()).line_height(Some(12.0))
}
// Every hover names the control's STATE and what a click DOES. The text and
// colour builders are pure, so tests read them without a window.

/// What a pad holds, in one line: "fugazi · seed 0042 · 7/8 · Maze 7919".
fn slot_summary(s: &params::SlotSnapshot) -> String {
    let mut parts = vec![s.style_name.clone(), format!("seed {:04}", s.seed)];
    if s.meter != 0 {
        parts.push(params::meter_label(s.meter));
    }
    if s.form > 0 {
        let shape = params::FORM_SHAPES.get(s.form as usize).map(|f| f.0).unwrap_or("?");
        parts.push(format!("{shape} {:04}", s.form_seed));
    } else if !s.song_name.is_empty() {
        parts.push(s.song_name.clone());
    } else {
        parts.push(format!("{} bars", s.bars));
    }
    parts.join(" · ")
}

/// What a pad is doing. Decides its colour and its hover text.
#[derive(Clone, Copy, Debug, PartialEq)]
enum PadState {
    Empty,
    /// Stored, its pattern still being generated: a click queues it.
    Generating,
    Ready,
    Playing,
}

fn pad_state(filled: bool, ready: bool, active: bool) -> PadState {
    if active {
        PadState::Playing
    } else if ready {
        PadState::Ready
    } else if filled {
        PadState::Generating
    } else {
        PadState::Empty
    }
}

/// A pad's fill. The `?` legend paints with this same function, so the
/// legend cannot drift from the pads.
fn pad_fill(state: PadState, queued_blink: bool) -> egui::Color32 {
    if queued_blink {
        return ACCENT_B;
    }
    match state {
        PadState::Empty => PANEL,
        PadState::Generating => GRID_FAINT,
        PadState::Ready => GRID_MID,
        PadState::Playing => ACCENT_A,
    }
}

/// Hover text for pad `n` (1-based): what it holds and what a click does now.
fn pad_hint(n: usize, state: PadState, queued: bool, armed: bool, holds: Option<&str>) -> String {
    let holds = holds.unwrap_or("");
    if armed {
        return if holds.is_empty() {
            format!("pad {n}: click to store what is playing here")
        } else {
            format!("pad {n}: click to replace ({holds}) with what is playing")
        };
    }
    if queued {
        return format!("pad {n}: {holds}. Takes over at the next bar");
    }
    match state {
        PadState::Empty => format!("pad {n} is empty. Arm STORE, then click here to store what is playing"),
        PadState::Generating => format!(
            "pad {n}: {holds}. Still generating: click now and it plays at the next bar once ready \
             · right-click: clear"
        ),
        PadState::Ready => format!("pad {n}: {holds}. Click: play from the next bar · right-click: clear"),
        PadState::Playing => format!(
            "pad {n} is playing: {holds}. Click another pad to switch at the next bar. \
             Touching any knob or stepper leaves the bank"
        ),
    }
}

/// The `?` overlay: the bank's two steps and the pad colours. Toggled from
/// the header; drawn over the grid, never moving the layout underneath.
/// ponytail: sized for the default 720x440, where it just fits; a window
/// dragged smaller clips its last rows. Upgrade: a ScrollArea inside the
/// frame, if anyone keeps the window small.
fn help_overlay(ctx: &egui::Context, open: &mut bool, top_left: egui::Pos2, width: f32) {
    if !*open {
        return;
    }
    egui::Area::new(egui::Id::new("drumgen-help"))
        .order(egui::Order::Foreground)
        .fixed_pos(top_left)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(BG)
                .stroke(egui::Stroke::new(2.0f32, ACCENT_B))
                .inner_margin(egui::Margin::same(10))
                .show(ui, |ui| {
                    ui.set_width(width);
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("THE BANK: BUILD, THEN JAM").color(ACCENT_B));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("CLOSE").clicked() {
                                *open = false;
                            }
                        });
                    });
                    let row = |ui: &mut egui::Ui, head: &str, body: &str| {
                        ui.horizontal_top(|ui| {
                            ui.add_sized(
                                [72.0, 12.0],
                                egui::Label::new(egui::RichText::new(head).color(ACCENT_A)),
                            );
                            ui.add(egui::Label::new(tip(body).color(TEXT)).wrap());
                        });
                    };
                    row(ui, "1 BUILD", "Dial a sound with the knobs (style, seed, song or form), \
                        arm STORE, click a pad. Up to 16 sounds.");
                    row(ui, "2 JAM", "Click a pad, or play MIDI notes 36 to 51 (C1 up) into the \
                        track: it takes over at the next bar.");
                    row(ui, "LEAVE", "Touch any knob or stepper and the knobs play again. Tempo \
                        changes keep the bank (pads re-bake).");
                    row(ui, "KEEP", "Pads save with the project and regenerate from their seeds.");
                    row(ui, "TWO DICE", "DICE rolls the notes. FORM SEED rolls the song form.");
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.add_sized(
                            [72.0, 12.0],
                            egui::Label::new(egui::RichText::new("PADS").color(ACCENT_A)),
                        );
                        for (state, blink, name) in [
                            (PadState::Empty, false, "empty"),
                            (PadState::Generating, false, "stored, generating"),
                            (PadState::Ready, false, "ready"),
                            (PadState::Playing, false, "playing"),
                            (PadState::Ready, true, "next bar (blinks)"),
                        ] {
                            let (r, _) = ui.allocate_exact_size(egui::vec2(16.0, 12.0), egui::Sense::hover());
                            ui.painter().rect_filled(r, egui::CornerRadius::ZERO, pad_fill(state, blink));
                            if state == PadState::Playing || blink {
                                ui.painter().rect_stroke(
                                    r,
                                    egui::CornerRadius::ZERO,
                                    egui::Stroke::new(2.0_f32, TEXT),
                                    egui::StrokeKind::Inside,
                                );
                            }
                            ui.label(egui::RichText::new(name).color(DIM));
                        }
                    });
                });
        });
}

/// Horizon strip: the current bar and the next three, in miniature, with a
/// moving playhead cursor — what's playing and what's coming, at a glance.
/// When stopped, the strip starts at `start_bar` and shows no cursor.
fn horizon_strip(ui: &mut egui::Ui, pattern: &Pattern, start_bar: usize, ph_tick: i64) {
    let total_bars = pattern.bar_starts.len().saturating_sub(1).max(1);
    let n = 4.min(total_bars);
    let cell_h = 6.0;
    let label_h = 11.0;
    let gap = 6.0;
    let avail = ui.available_width();
    let block_w = ((avail - gap * (n as f32 - 1.0)) / n as f32).floor();
    let h = label_h + cell_h * GRID_LANES as f32;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(avail, h), egui::Sense::hover());
    resp.on_hover_text(tip("the bar playing now and the next three, in miniature; the line is the playhead"));
    let painter = ui.painter_at(rect);
    let t = if ph_tick >= 0 { ph_tick.rem_euclid(pattern.total_ticks.max(1)) } else { -1 };

    for i in 0..n {
        let bar = (start_bar + i) % total_bars; // bounded — never >= total_bars
        let (grid, ncols) = grid_cells(pattern, bar);
        let x0 = rect.left() + i as f32 * (block_w + gap);
        // Bar number label; the playhead bar gets the accent.
        let is_now = t >= 0
            && pattern.bar_starts[bar] <= t
            && t < pattern.bar_starts[bar + 1];
        painter.text(
            egui::pos2(x0, rect.top()),
            egui::Align2::LEFT_TOP,
            format!("{}", bar + 1),
            egui::FontId::proportional(8.0),
            if is_now { ACCENT_A } else { DIM },
        );
        let cw = (block_w / ncols as f32).floor().max(2.0);
        let gy = rect.top() + label_h;
        for lane in 0..GRID_LANES {
            for col in 0..ncols {
                let v = grid[lane][col];
                let color = if v >= 96 {
                    ACCENT_A
                } else if v >= 1 {
                    GRID_MID
                } else {
                    PANEL
                };
                painter.rect_filled(
                    egui::Rect::from_min_size(
                        egui::pos2(x0 + col as f32 * cw, gy + lane as f32 * cell_h),
                        egui::vec2(cw - 1.0, cell_h - 1.0),
                    ),
                    0.0,
                    color,
                );
            }
        }
        // Playhead cursor: a 2px butter line sweeping through the NOW bar.
        if is_now {
            let (bs, be) = (pattern.bar_starts[bar], pattern.bar_starts[bar + 1]);
            let frac = (t - bs) as f32 / (be - bs).max(1) as f32;
            let x = x0 + frac * cw * ncols as f32;
            painter.line_segment(
                [egui::pos2(x, gy), egui::pos2(x, gy + cell_h * GRID_LANES as f32)],
                egui::Stroke::new(2.0f32, ACCENT_B),
            );
        }
    }
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
    // The viewed bar's section (song mode only; empty in loop mode) AND the
    // cell actually playing there. The two are not the same thing: the section
    // type is what the song form ASKED for, and 22 of the style pools have no
    // blast cell at all — labelling an unwound bar "BLAST" because Labyrinth
    // said so is the header lying about its own output.
    let section = {
        let mut cum = 0;
        pattern
            .sections
            .iter()
            .find(|(_, bars, _)| {
                cum += bars;
                bar_number <= cum
            })
            .map(|(name, _, cell)| (name.to_uppercase(), cell.clone()))
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
            let pager = ui.button(label).on_hover_text(tip(if playing {
                "click: cycle BAR/NOW/NEXT follow"
            } else {
                "click: view next bar"
            }));
            if pager.clicked() {
                if playing {
                    *follow = (*follow + 1) % 3;
                } else {
                    *view_bar = (*view_bar + 1) % total_bars;
                }
            }
            if let Some((sec, cell)) = section {
                // Name the CELL — that is what you are hearing. The section type
                // rides along in the telegraph, which is form navigation.
                if cell.is_empty() {
                    ui.label(egui::RichText::new(sec).color(DIM));
                } else {
                    ui.label(egui::RichText::new(&cell).color(DIM)).on_hover_text(tip(
                        format!("{} section, playing cell '{}'", sec, cell),
                    ));
                }
            }
        });
    });

    // Beat-pulse columns: one beat = 4 sixteenths in /4 meters, 2 in /8.
    let pulse = if den == 8 { 2 } else { 4 };
    let gutter = 30.0;
    let avail = ui.available_width();
    // Grow into whatever the user dragged: the detail grid absorbs all
    // remaining height (minus the panel's bottom margin) and widens up to a
    // chunky cap. At the default 720x440 this lands near the old 13px rows.
    let cell_w = ((avail - gutter) / ncols as f32).floor().clamp(6.0, 44.0);
    let remaining = (ui.available_height() - 10.0).max(0.0);
    let cell_h = (remaining / GRID_LANES as f32).floor().clamp(13.0, 44.0);
    let w = gutter + cell_w * ncols as f32;
    let h = cell_h * GRID_LANES as f32;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    resp.on_hover_text(tip(format!(
        "bar {} in detail: one column per sixteenth, brighter is louder. The button above pages \
         bars, or follows the playhead while playing",
        *view_bar + 1
    )));
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

    fn slot(form: i32, song: &str, meter: i32) -> params::SlotSnapshot {
        params::SlotSnapshot {
            style_name: "fugazi".into(),
            style_index: 0,
            humanize: 0.4,
            bars: 4,
            seed: 42,
            swing: 0.0,
            meter,
            fill: 0,
            song: if song.is_empty() { 0 } else { 1 },
            song_name: song.into(),
            form,
            form_name: params::form_key(form).into(),
            form_seed: 7919,
        }
    }

    #[test]
    fn a_pad_summary_names_what_it_will_play() {
        assert_eq!(slot_summary(&slot(0, "", 0)), "fugazi · seed 0042 · 4 bars");
        assert_eq!(slot_summary(&slot(0, "Verse/Chor", 6)), "fugazi · seed 0042 · 7/8 · Verse/Chor");
        // A form wins over the song it was stored next to, as in playback.
        assert_eq!(slot_summary(&slot(5, "Verse/Chor", 0)), "fugazi · seed 0042 · Maze 7919");
    }

    /// Each pad state has its own colour, the queued blink is distinct from
    /// all of them, and the ? legend draws through the same function.
    #[test]
    fn every_pad_state_reads_differently() {
        let states = [PadState::Empty, PadState::Generating, PadState::Ready, PadState::Playing];
        let mut colours: Vec<egui::Color32> = states.iter().map(|&s| pad_fill(s, false)).collect();
        colours.push(pad_fill(PadState::Ready, true));
        let mut unique = colours.clone();
        unique.sort_by_key(|c| c.to_array());
        unique.dedup();
        assert_eq!(unique.len(), colours.len(), "{colours:?}");
        assert_eq!(pad_state(true, true, true), PadState::Playing);
        assert_eq!(pad_state(true, true, false), PadState::Ready);
        assert_eq!(pad_state(true, false, false), PadState::Generating);
        assert_eq!(pad_state(false, false, false), PadState::Empty);
    }

    /// The hover says what the pad holds and what a click will do now.
    #[test]
    fn a_pad_hover_says_what_a_click_does_now() {
        let holds = Some("fugazi · seed 0042 · 4 bars");
        let ready = pad_hint(3, PadState::Ready, false, false, holds);
        assert!(ready.starts_with("pad 3: fugazi") && ready.contains("play from the next bar"), "{ready}");
        let gen = pad_hint(3, PadState::Generating, false, false, holds);
        assert!(gen.contains("Still generating"), "{gen}");
        let playing = pad_hint(3, PadState::Playing, false, false, holds);
        assert!(playing.contains("leaves the bank"), "{playing}");
        assert!(pad_hint(3, PadState::Ready, true, false, holds).contains("next bar"));
        assert!(pad_hint(3, PadState::Empty, false, false, None).contains("Arm STORE"));
        // Armed: storing, and it warns before replacing.
        assert!(pad_hint(3, PadState::Empty, false, true, None).contains("store what is playing"));
        assert!(pad_hint(3, PadState::Ready, false, true, holds).contains("replace (fugazi"));
    }

    /// Every non-ASCII character this editor shows must exist in a loaded
    /// font. A missing one renders as an empty box, and nothing else catches
    /// it: the DICE button's die (U+2684) and the telegraph's arrow (U+25B8)
    /// were boxes in Bitwig for months. Scans the string literals of this
    /// file, so a new glyph is checked the day it is typed.
    #[test]
    fn every_glyph_the_editor_shows_is_in_a_font() {
        let ctx = egui::Context::default();
        install_theme(&ctx);
        let mut missing = Vec::new();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            let font = egui::FontId::new(8.0, egui::FontFamily::Proportional);
            for line in include_str!("editor.rs").lines() {
                let code = line.trim_start();
                if code.starts_with("//") {
                    continue;
                }
                // Characters inside "..." on this line.
                for (i, lit) in code.split('"').enumerate() {
                    if i % 2 == 1 {
                        for c in lit.chars().filter(|c| !c.is_ascii()) {
                            if !ctx.fonts(|f| f.has_glyph(&font, c)) {
                                missing.push(c);
                            }
                        }
                    }
                }
            }
        });
        missing.sort();
        missing.dedup();
        assert!(missing.is_empty(), "glyphs with no font: {missing:?}");
    }

    #[test]
    fn bank_telegraph_outranks_the_section_line() {
        // Nothing queued: the bank stays out of the way entirely.
        assert_eq!(telegraph_bank(-1), None);
        // Queued: 1-based pad number, always hot.
        assert_eq!(
            telegraph_bank(6),
            Some(("▶ SLOT 7 NEXT BAR".to_string(), true))
        );
        // It must win over a live section countdown — the pad press is the
        // thing the player just did, and the swap is what happens next.
        let sections = vec![("verse".to_string(), 4, "cell".to_string())];
        let section_line = telegraph(&sections, 4, 1, 0);
        assert!(section_line.is_some(), "the section line exists to be beaten");
        let shown = telegraph_bank(0).or(section_line);
        assert_eq!(shown.unwrap().0, "▶ SLOT 1 NEXT BAR");
    }

    fn test_pattern(events: Vec<MidiEvent>) -> Pattern {
        Pattern {
            events,
            total_ticks: 2 * 4 * PPQ,
            bar_starts: vec![0, 4 * PPQ, 8 * PPQ],
            time_signatures: vec![TimeSigEntry { bar_start: 1, bar_end: 2, numerator: 4, denominator: 4 }],
            generation: 0,
            content_key: 0,
            seed: 0,
            tempo: 120.0,
            style_name: "test".into(),
            song_name: String::new(),
            arrangement: String::new(),
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
    fn ph_bar_of_reduces_ticks_into_the_snapshot() {
        let pat = test_pattern(vec![on(0, 36, 100)]); // 2 bars of 4/4, 3840 ticks
        assert_eq!(ph_bar_of(-1, &pat), -1);
        assert_eq!(ph_bar_of(0, &pat), 0);
        assert_eq!(ph_bar_of(1919, &pat), 0);
        assert_eq!(ph_bar_of(1920, &pat), 1);
        // Tick published against a LONGER audio-side pattern reduces safely
        // into this snapshot (the pending-swap guard).
        assert_eq!(ph_bar_of(3840, &pat), 0);
        assert_eq!(ph_bar_of(5000, &pat), 0);
        assert_eq!(ph_bar_of(5761, &pat), 1);
    }

    #[test]
    fn telegraph_loop_mode_counts_down_to_fill() {
        let none: &[(String, i32, String)] = &[];
        assert_eq!(
            telegraph(none, 4, 0, 4),
            Some(("FILL IN 3".to_string(), false))
        );
        assert_eq!(telegraph(none, 4, 2, 4), Some(("FILL IN 1".to_string(), true)));
        assert_eq!(telegraph(none, 4, 3, 4), Some(("▶ FILL NOW".to_string(), true)));
        // FILL off, no sections: nothing to telegraph.
        assert_eq!(telegraph(none, 4, 1, 0), None);
    }

    #[test]
    fn telegraph_song_mode_walks_sections() {
        let sections = vec![
            ("intro".to_string(), 2, String::new()),
            ("build".to_string(), 2, String::new()),
            ("blast".to_string(), 4, String::new()),
        ];
        assert_eq!(
            telegraph(&sections, 8, 0, 0),
            Some(("▶ INTRO NOW".to_string(), true))
        );
        assert_eq!(
            telegraph(&sections, 8, 1, 0),
            Some(("INTRO ▶ BUILD IN 1".to_string(), true))
        );
        assert_eq!(
            telegraph(&sections, 8, 4, 0),
            Some(("▶ BLAST NOW".to_string(), true))
        );
        // Mid-blast: next wraps to the top of the form.
        assert_eq!(
            telegraph(&sections, 8, 5, 0),
            Some(("BLAST ▶ INTRO IN 3".to_string(), false))
        );
    }

    #[test]
    fn telegraph_wraps_playhead_past_pattern_end() {
        let sections = vec![("intro".to_string(), 2, String::new()), ("blast".to_string(), 2, String::new())];
        // ph 5 wraps to bar 2 (0-based 1) -> intro's last bar.
        assert_eq!(telegraph(&sections, 4, 5, 0), telegraph(&sections, 4, 1, 0));
    }

    #[test]
    fn telegraph_stopped_is_silent() {
        let sections = vec![("intro".to_string(), 2, String::new())];
        assert_eq!(telegraph(&sections, 2, -1, 4), None);
        let none: &[(String, i32, String)] = &[];
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

    /// Every bar the plugin can be in must get its true column count. The cap
    /// was 24, so 7/4 and anything wider silently piled hits into one cell.
    #[test]
    fn grid_covers_every_reachable_meter_without_clamping() {
        for n in 1..=15i64 {
            for d in [4i64, 8] {
                let bar = n * PPQ * 4 / d;
                let mut pat = test_pattern(vec![on(0, 36, 100)]);
                pat.bar_starts = vec![0, bar, 2 * bar];
                pat.time_signatures = vec![TimeSigEntry {
                    bar_start: 1, bar_end: 2, numerator: n as i32, denominator: d as i32,
                }];
                let (_, ncols) = grid_cells(&pat, 0);
                let want = ((bar + PPQ / 4 - 1) / (PPQ / 4)) as usize;
                assert_eq!(ncols, want, "{n}/{d} was clamped");
            }
        }
    }

    /// Late hits in a wide bar land on their own column, not the last one.
    #[test]
    fn a_hit_late_in_a_wide_bar_keeps_its_column() {
        // 7/4 = 28 sixteenths; a snare on the 28th sixteenth's neighbor.
        let bar = 7 * PPQ;
        let sixteenth = PPQ / 4;
        let mut pat = test_pattern(vec![on(26 * sixteenth, 38, 100), on(27 * sixteenth, 36, 100)]);
        pat.bar_starts = vec![0, bar, 2 * bar];
        pat.time_signatures =
            vec![TimeSigEntry { bar_start: 1, bar_end: 2, numerator: 7, denominator: 4 }];
        let (grid, ncols) = grid_cells(&pat, 0);
        assert_eq!(ncols, 28);
        assert_eq!(grid[4][26], 100, "snare on column 26");
        assert_eq!(grid[5][27], 100, "kick on column 27");
        assert_eq!(grid[4][23], 0, "nothing piled into the old clamp column");
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
    /// Same, for the FORM SEED box.
    form_seed_acc: f32,
    /// Which bar the step grid shows (0-based; clamped to the pattern).
    view_bar: usize,
    /// Grid follow mode while the transport runs: 0 = MANUAL (pager), 1 = NOW
    /// (playhead bar), 2 = NEXT (upcoming bar — the jam default).
    follow: u8,
    /// STORE is armed: the next pad click captures the current sound instead of
    /// triggering. Deliberately not persisted — arming is a gesture, and a
    /// project that reopened armed would eat the first pad press.
    store_armed: bool,
    /// The `?` overlay is open. Not persisted: help is a moment, not a mode.
    help_open: bool,
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
    playhead_tick: Arc<AtomicI64>,
) -> Option<Box<dyn Editor>> {
    let egui_state = params.editor_state.clone();
    create_egui_editor(
        egui_state,
        UiState::new(),
        |ctx, _| install_theme(ctx),
        move |ctx, setter, ui_state| {
            let pattern = pattern_view.lock().unwrap_or_else(|e| e.into_inner()).clone();
            let ph_tick = playhead_tick.load(Ordering::Relaxed);
            // 16ms while playing (smooth cursor), lazy 100ms poll when stopped.
            // A queued pad blinks, so it needs the fast rate even when stopped.
            let queued_now = params.bank.queued.load(Ordering::Relaxed) >= 0;
            ctx.request_repaint_after(std::time::Duration::from_millis(
                if ph_tick >= 0 || queued_now { 16 } else { 100 },
            ));

            // ResizableWindow: same CentralPanel underneath (panel_fill == BG,
            // default margin 8) plus a drag grip in the bottom-right corner.
            ResizableWindow::new("drumgen-window")
                .min_size(egui::Vec2::new(480.0, 320.0))
                .show(ctx, params.editor_state.as_ref(), |ui| {
                    // What is playing now, for the hover texts below.
                    let loop_bars = pattern.bar_starts.len().saturating_sub(1).max(1);
                    let bank_active = params.bank.active.load(Ordering::Relaxed);

                    // Header: logo, help, style picker.
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("DRUMGEN").heading().color(ACCENT_A));
                        let help = ui
                            .add(egui::Button::new(
                                egui::RichText::new("?").color(if ui_state.help_open { BG } else { TEXT }),
                            )
                            .fill(if ui_state.help_open { ACCENT_B } else { PANEL }))
                            .on_hover_text(tip("how the bank works, and what the pad colours mean"));
                        if help.clicked() {
                            ui_state.help_open = !ui_state.help_open;
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("▶").clicked() {
                                step_int(setter, &params.style, params.style.value(), 1, n_styles as i32);
                            }
                            // While a pad plays, the STYLE param is not what is
                            // sounding — say so instead of showing a stale name
                            // as if it described the audio.
                            let (label, colour) = if bank_active >= 0 {
                                let song = if pattern.song_name.is_empty() {
                                    String::new()
                                } else {
                                    format!(" · {}", pattern.song_name)
                                };
                                (
                                    format!("PAD {} · {}{song}", bank_active + 1, pattern.style_name),
                                    ACCENT_A,
                                )
                            } else {
                                (params.style.to_string(), TEXT)
                            };
                            let style_hint = if bank_active >= 0 {
                                format!(
                                    "pad {} is playing, so the knobs are not what you hear. \
                                     Touch any knob or stepper to leave the bank",
                                    bank_active + 1
                                )
                            } else {
                                format!(
                                    "style {} of {}: one pool of hand-written cells. ◀ ▶ to change",
                                    params.style.value() + 1,
                                    n_styles
                                )
                            };
                            ui.add_sized(
                                [180.0, 24.0],
                                egui::Label::new(egui::RichText::new(label).color(colour)),
                            )
                            .on_hover_text(tip(style_hint));
                            if ui.button("◀").clicked() {
                                step_int(setter, &params.style, params.style.value(), -1, n_styles as i32);
                            }
                        });
                    });
                    ui.separator();

                    // Control row: knobs + steppers.
                    ui.horizontal_top(|ui| {
                        let hz = params.humanize.value() * 100.0;
                        pixel_knob(ui, setter, "HUMANIZE", &params.humanize, &format!(
                            "humanize {hz:.0}%: velocity and timing feel, flams, ghost notes. \
                             0% is a machine. Drag up/down"
                        ));
                        let sw = params.swing.value() * 100.0;
                        pixel_knob(ui, setter, "SWING", &params.swing, &format!(
                            "swing {sw:.0}%: 0% is straight, 50% full triplet, beyond leans \
                             dotted. Drag up/down"
                        ));
                        ui.add_space(4.0);

                        // In song mode (a SONG or a generated FORM) the
                        // arrangement string owns length and fills; BARS and
                        // FILL go dead. METER stays live as the home meter
                        // (and on Auto, a form may turn meters).
                        let form_on = params.form.value() > 0;
                        let song_off = !form_on && params::song_str(params.song.value()).is_empty();
                        ui.add_enabled_ui(song_off, |ui| {
                            let db = stepper(
                                ui, "BARS", &params.bars.value().to_string(), 32.0,
                                &format!("loop length: {} bars (1 to 16). FILL can stretch the loop \
                                          to hold its cycle; it plays {loop_bars} now", params.bars.value()),
                                &format!("the song or form sets the length ({loop_bars} bars now)"),
                            );
                            if db != 0 {
                                let next = (params.bars.value() + db).clamp(1, 16);
                                setter.begin_set_parameter(&params.bars);
                                setter.set_parameter(&params.bars, next);
                                setter.end_set_parameter(&params.bars);
                            }
                        });

                        let meter_hint = if params.meter.value() == 0 {
                            format!("Auto: follows the host's time signature{}. ◀ ▶ forces a meter",
                                if form_on { ", and a form may turn sections into the style's other meters" } else { "" })
                        } else {
                            format!("forced {}: plays it whatever the host says{}. ◀ ▶ back to Auto",
                                params.meter, if form_on { ", and holds the whole form" } else { "" })
                        };
                        let dm = stepper(ui, "METER", &params.meter.to_string(), 40.0, &meter_hint, "");
                        if dm != 0 {
                            step_int(setter, &params.meter, params.meter.value(), dm, params::METERS.len() as i32);
                        }

                        ui.add_enabled_ui(song_off, |ui| {
                            let every = params::fill_of(params.fill.value());
                            let fill_hint = if every == 0 {
                                "no fills. ◀ ▶ for a fill every 8, 4 or 2 bars".to_string()
                            } else {
                                format!("a fill closes every {every} bars. The loop plays {loop_bars} \
                                         bars so the cycle fits")
                            };
                            let df = stepper(ui, "FILL", &params.fill.to_string(), 56.0, &fill_hint,
                                "the song or form places its own fills");
                            if df != 0 {
                                step_int(setter, &params.fill, params.fill.value(), df, params::FILLS.len() as i32);
                            }
                        });
                    });

                    ui.add_space(4.0);

                    // SONG, FORM + its dice, DICE hero, SAVE, last save result.
                    ui.horizontal(|ui| {
                        // A generated FORM overrides SONG: grey SONG out
                        // rather than let it look like it is playing.
                        let form_on = params.form.value() > 0;
                        ui.add_enabled_ui(!form_on, |ui| {
                            let song_hint = match params::song_str(params.song.value()) {
                                "" => "Off: a loop. ◀ ▶ picks a song form: 9 built in, plus yours in \
                                       ~/.config/drumgen/songs.txt (restart the DAW to reload)".to_string(),
                                arr => format!("{}: {arr}", params.song),
                            };
                            let ds = stepper(
                                ui, "SONG", &params.song.to_string(), 80.0, &song_hint,
                                "FORM is on and overrides SONG. Set FORM to Off to use songs",
                            );
                            if ds != 0 {
                                step_int(setter, &params.song, params.song.value(), ds, params::n_songs() as i32);
                            }
                        });
                        ui.add_space(8.0);
                        let form_hint = if form_on {
                            format!("{}: {}. FORM SEED rolls a new one; overrides SONG",
                                params.form, params::form_about(params.form.value()))
                        } else {
                            "Off. ◀ ▶ picks a shape and generates a song form from it, rolled \
                             with FORM SEED. Overrides SONG".to_string()
                        };
                        let dform = stepper(ui, "FORM", &params.form.to_string(), 72.0, &form_hint, "");
                        if dform != 0 {
                            step_int(setter, &params.form, params.form.value(), dform, params::FORM_SHAPES.len() as i32);
                        }
                        ui.add_enabled_ui(form_on, |ui| {
                            form_seed_box(
                                ui, setter, &params.form_seed, &params.form.to_string(),
                                &mut ui_state.form_seed_acc,
                            );
                        });
                        let dice = ui
                            .add_sized(
                                [80.0, 40.0],
                                egui::Button::new(egui::RichText::new("DICE").color(BG)).fill(ACCENT_A),
                            )
                            .on_hover_text(tip(format!(
                                "roll new notes (seed {:04} now). The style, song and form stay; \
                                 FORM SEED rolls the form",
                                params.seed.value()
                            )));
                        if dice.clicked() {
                            let next = params::dice_roll(params.seed.value());
                            setter.begin_set_parameter(&params.seed);
                            setter.set_parameter(&params.seed, next);
                            setter.end_set_parameter(&params.seed);
                        }
                        seed_drag(ui, setter, &params.seed, &mut ui_state.seed_acc);
                        let save = ui.button("SAVE .MID").on_hover_text(tip(format!(
                            "write these {loop_bars} bars to ~/drumgen_output, named after the style, \
                             seed and song. An on_save hook also renders a score (NOTATION.md)"
                        )));
                        if save.clicked() {
                            ui_state.save_msg = match export::save_pattern(&pattern) {
                                Ok(path) => {
                                    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
                                    if export::save_hook_installed() {
                                        format!("SAVED {name} ▶ score")
                                    } else {
                                        format!("SAVED {name}")
                                    }
                                }
                                Err(e) => format!("SAVE FAILED: {e}"),
                            };
                        }
                        if !ui_state.save_msg.is_empty() {
                            ui.label(egui::RichText::new(&ui_state.save_msg).color(DIM));
                        }
                    });

                    ui.add_space(4.0);

                    // The bank: 16 pads, stored sounds, triggered live.
                    let blink_on = (ui.input(|i| i.time) * 4.0) as i64 % 2 == 0;
                    bank_row(ui, &params, &mut ui_state.store_armed, blink_on);

                    // The ? overlay sits under the pads, so the pads it
                    // explains stay in view while it is open.
                    let help_at = ui.cursor().min;
                    let help_w = (ui.available_width() - 24.0).max(200.0);
                    help_overlay(ui.ctx(), &mut ui_state.help_open, help_at, help_w);

                    ui.add_space(4.0);

                    // Telegraph: what the drummer does next, readable mid-riff.
                    let ph_bar = ph_bar_of(ph_tick, &pattern);
                    let total_bars = pattern.bar_starts.len().saturating_sub(1).max(1);
                    let queued = params.bank.queued.load(Ordering::Relaxed);
                    if let Some((line, hot)) = telegraph_bank(queued).or_else(|| telegraph(
                        &pattern.sections,
                        total_bars,
                        ph_bar,
                        params::fill_of(params.fill.value()),
                    )) {
                        ui.label(
                            egui::RichText::new(line)
                                .color(if hot { ACCENT_A } else { ACCENT_B }),
                        )
                        .on_hover_text(tip(
                            "what comes next, readable mid-riff: the section countdown in a song \
                             or form, the next fill in a loop, a pad waiting for the bar",
                        ));
                    }

                    // Horizon: now + the next three bars, cursor sweeping.
                    let horizon_start = if ph_bar >= 0 {
                        ph_bar as usize % total_bars
                    } else {
                        ui_state.view_bar.min(total_bars - 1)
                    };
                    horizon_strip(ui, &pattern, horizon_start, ph_tick);
                    ui.add_space(4.0);

                    // Step grid: follows the playhead while playing (NOW/NEXT),
                    // manual pager when stopped.
                    step_grid(ui, &pattern, &mut ui_state.view_bar, ph_bar, &mut ui_state.follow);
                });
        },
    )
}
