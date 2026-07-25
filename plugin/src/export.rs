//! SAVE .MID — hand-rolled Standard MIDI File (format 0) writer.
//!
//! ~100 lines instead of a midly/rfd dependency: the pattern is already a
//! sorted event list at PPQ 480, so encoding is a straight transcription.
//! Files land in `~/drumgen_output/` with an auto-incrementing suffix — no
//! file dialog, drag the file from there into the DAW.

use std::io::Write;
use std::path::PathBuf;

use crate::engine::midi_math::PPQ;
use crate::pattern::Pattern;
use crate::MIDI_CHANNEL;

/// Append a MIDI variable-length quantity.
fn push_vlq(out: &mut Vec<u8>, mut v: u64) {
    let mut buf = [0u8; 5];
    let mut i = 4;
    buf[4] = (v & 0x7f) as u8;
    v >>= 7;
    while v > 0 {
        i -= 1;
        buf[i] = 0x80 | (v & 0x7f) as u8;
        v >>= 7;
    }
    out.extend_from_slice(&buf[i..]);
}

/// Encode the pattern as a complete format-0 SMF byte stream.
fn encode_smf(pattern: &Pattern) -> Vec<u8> {
    // Collect (tick, bytes) message list: metas first so a stable sort keeps
    // them ahead of notes at the same tick.
    let mut msgs: Vec<(i64, Vec<u8>)> = Vec::with_capacity(pattern.events.len() + 8);

    let name = format!("drumgen {} seed {:04}", pattern.style_name, pattern.seed);
    let mut m = vec![0xFF, 0x03, name.len().min(127) as u8];
    m.extend_from_slice(&name.as_bytes()[..name.len().min(127)]);
    msgs.push((0, m));

    let bpm = if pattern.tempo > 0.0 { pattern.tempo } else { 120.0 };
    let uspq = (60_000_000.0 / bpm).round() as u32;
    msgs.push((0, vec![0xFF, 0x51, 0x03, (uspq >> 16) as u8, (uspq >> 8) as u8, uspq as u8]));

    for ts in &pattern.time_signatures {
        let tick = pattern
            .bar_starts
            .get((ts.bar_start - 1).max(0) as usize)
            .copied()
            .unwrap_or(0);
        let dd = (ts.denominator.max(1) as f64).log2().round() as u8;
        msgs.push((tick, vec![0xFF, 0x58, 0x04, ts.numerator.max(1) as u8, dd, 24, 8]));
    }

    for ev in &pattern.events {
        let status = if ev.is_note_on { 0x90 } else { 0x80 } | MIDI_CHANNEL;
        msgs.push((ev.tick.max(0), vec![status, ev.note & 0x7F, ev.velocity & 0x7F]));
    }

    msgs.push((pattern.total_ticks.max(0), vec![0xFF, 0x2F, 0x00]));
    msgs.sort_by_key(|(tick, _)| *tick);

    let mut track = Vec::with_capacity(msgs.len() * 4);
    let mut prev = 0i64;
    for (tick, bytes) in msgs {
        push_vlq(&mut track, (tick - prev).max(0) as u64);
        track.extend_from_slice(&bytes);
        prev = tick;
    }

    let mut out = Vec::with_capacity(track.len() + 22);
    out.extend_from_slice(b"MThd");
    out.extend_from_slice(&6u32.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes()); // format 0
    out.extend_from_slice(&1u16.to_be_bytes()); // one track
    out.extend_from_slice(&(PPQ as u16).to_be_bytes());
    out.extend_from_slice(b"MTrk");
    out.extend_from_slice(&(track.len() as u32).to_be_bytes());
    out.extend_from_slice(&track);
    out
}

fn output_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("drumgen_output")
}

/// Write the pattern to `~/drumgen_output/`, never overwriting an existing
/// file (auto `_1`, `_2`, ... suffix). Returns the written path. All labels
/// (filename seed, track-name meta) come from the pattern itself, so they
/// always describe the notes actually written.
pub fn save_pattern(pattern: &Pattern) -> std::io::Result<PathBuf> {
    let dir = output_dir();
    std::fs::create_dir_all(&dir)?;

    let bars = pattern.bar_starts.len().saturating_sub(1).max(1);
    let meter = pattern
        .time_signatures
        .first()
        .map(|ts| (ts.numerator, ts.denominator))
        .unwrap_or((4, 4));
    let meter_suffix = if meter == (4, 4) { String::new() } else { format!("_{}_{}", meter.0, meter.1) };
    let base = format!(
        "{}_{}bpm_{:04}{}_{}bars",
        pattern.style_name, pattern.tempo.round() as i64, pattern.seed, meter_suffix, bars
    );

    let mut path = dir.join(format!("{base}.mid"));
    let mut n = 0;
    while path.exists() {
        n += 1;
        path = dir.join(format!("{base}_{n}.mid"));
    }

    let bytes = encode_smf(pattern);
    let mut f = std::fs::File::create(&path)?;
    f.write_all(&bytes)?;
    run_save_hook(&path);
    Ok(path)
}

/// Fire-and-forget user hook: if `~/.config/drumgen/on_save` exists and is
/// executable it is spawned with the saved .mid path as its argument. This is
/// how SAVE .MID grows superpowers (e.g. auto-render a MuseScore-ready score
/// via notation.py) without the plugin ever depending on Python. Runs on the
/// GUI thread, detached — never blocks, failures only log.
fn run_save_hook(path: &std::path::Path) -> bool {
    let Some(home) = std::env::var_os("HOME") else { return false };
    let hook = PathBuf::from(home).join(".config/drumgen/on_save");
    if !hook.is_file() {
        return false;
    }
    match std::process::Command::new(&hook).arg(path).spawn() {
        Ok(_) => true,
        Err(e) => {
            nih_plug::nih_log!("drumgen: on_save hook failed to spawn: {e}");
            false
        }
    }
}

/// Whether the on-save hook is installed (for the GUI's saved-message).
pub fn save_hook_installed() -> bool {
    std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join(".config/drumgen/on_save").is_file())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::midi_math::TimeSigEntry;
    use crate::pattern::MidiEvent;

    #[test]
    fn vlq_encoding_matches_spec() {
        for (v, expect) in [
            (0u64, vec![0x00u8]),
            (0x7F, vec![0x7F]),
            (0x80, vec![0x81, 0x00]),
            (0x4000, vec![0x81, 0x80, 0x00]),
        ] {
            let mut out = Vec::new();
            push_vlq(&mut out, v);
            assert_eq!(out, expect, "vlq({v:#x})");
        }
    }

    #[test]
    fn smf_bytes_are_well_formed() {
        let pattern = Pattern {
            events: vec![
                MidiEvent { tick: 0, note: 36, velocity: 100, is_note_on: true },
                MidiEvent { tick: 30, note: 36, velocity: 0, is_note_on: false },
                MidiEvent { tick: 960, note: 38, velocity: 90, is_note_on: true },
                MidiEvent { tick: 990, note: 38, velocity: 0, is_note_on: false },
            ],
            total_ticks: 1920,
            bar_starts: vec![0, 1920],
            time_signatures: vec![TimeSigEntry { bar_start: 1, bar_end: 1, numerator: 4, denominator: 4 }],
            generation: 0,
            seed: 12345,
            tempo: 160.0,
            style_name: "screamo".into(),
            cell_name: String::new(),
            sections: Vec::new(),
        };
        let bytes = encode_smf(&pattern);

        assert_eq!(&bytes[0..4], b"MThd");
        assert_eq!(u32::from_be_bytes(bytes[4..8].try_into().unwrap()), 6);
        assert_eq!(u16::from_be_bytes(bytes[8..10].try_into().unwrap()), 0); // format
        assert_eq!(u16::from_be_bytes(bytes[10..12].try_into().unwrap()), 1); // ntracks
        assert_eq!(u16::from_be_bytes(bytes[12..14].try_into().unwrap()), PPQ as u16);
        assert_eq!(&bytes[14..18], b"MTrk");
        let track_len = u32::from_be_bytes(bytes[18..22].try_into().unwrap()) as usize;
        assert_eq!(22 + track_len, bytes.len(), "track length must cover the rest of the file");
        assert_eq!(&bytes[bytes.len() - 3..], &[0xFF, 0x2F, 0x00], "must end with EndOfTrack");

        // Tempo meta: 160 BPM = 375000 us/quarter = 0x05B8D8.
        let tempo_pos = bytes.windows(3).position(|w| w == [0xFF, 0x51, 0x03]).expect("tempo meta");
        assert_eq!(&bytes[tempo_pos + 3..tempo_pos + 6], &[0x05, 0xB8, 0xD8]);

        // Time signature meta 4/4 -> nn=4 dd=2.
        let ts_pos = bytes.windows(3).position(|w| w == [0xFF, 0x58, 0x04]).expect("timesig meta");
        assert_eq!(&bytes[ts_pos + 3..ts_pos + 5], &[4, 2]);

        // First note-on: channel-10 status 0x99, kick, vel 100.
        assert!(bytes.windows(3).any(|w| w == [0x99, 36, 100]));
        // Matching note-off 0x89.
        assert!(bytes.windows(3).any(|w| w == [0x89, 36, 0]));
    }
}
