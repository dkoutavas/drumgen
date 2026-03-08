import streamlit as st
import json
import os
import tempfile
from pathlib import Path
from datetime import datetime

from cell_library import CELLS, STYLE_POOLS, STYLE_MAP, SECTION_PREFERENCES, list_cells
from assembler import assemble, assemble_arrangement
from midi_engine import write_midi, DEFAULT_PPQ
from preview import render_midi_to_wav, is_fluidsynth_available, find_soundfont
from midi_reader import midi_to_cell, save_cell, auto_tag_cell, validate_cell

st.set_page_config(page_title="drumgen", page_icon="\U0001f941", layout="wide", initial_sidebar_state="expanded")

# ── Persistent config ─────────────────────────────────────────────────────────

CONFIG_PATH = Path(__file__).parent / ".drumgen_config.json"
DEFAULT_OUTPUT = "/mnt/c/Users/diony/Documents/drumgen_output"


def load_config():
    if CONFIG_PATH.exists():
        return json.loads(CONFIG_PATH.read_text())
    return {"output_folder": DEFAULT_OUTPUT}


def save_config(config):
    CONFIG_PATH.write_text(json.dumps(config))


def wsl_to_windows_path(wsl_path):
    if wsl_path.startswith("/mnt/") and len(wsl_path) > 6 and wsl_path[5].isalpha() and wsl_path[6] == "/":
        drive = wsl_path[5].upper()
        rest = wsl_path[7:].replace("/", "\\")
        return f"{drive}:\\{rest}"
    return wsl_path


# ── Auto filename ─────────────────────────────────────────────────────────────

def auto_filename(style, tempo, bars, arrangement=False):
    if arrangement:
        return f"{style}_{tempo}bpm_arrangement.mid"
    return f"{style}_{tempo}bpm_{bars}bars.mid"


# ── Pattern preview ───────────────────────────────────────────────────────────

def build_pattern_preview(events, time_signatures, tempo, max_bars=4):
    ppq = DEFAULT_PPQ
    ts = time_signatures[0]
    num, den = ts["numerator"], ts["denominator"]
    beat_ticks = ppq * 4 // den
    bar_ticks = num * beat_ticks
    slots_per_beat = 4  # sixteenth note resolution
    slot_ticks = beat_ticks // slots_per_beat
    total_slots = num * slots_per_beat

    # Group events by bar
    bars_data = {}
    for abs_tick, instrument, velocity in events:
        bar_num = abs_tick // bar_ticks + 1
        if bar_num > max_bars:
            continue
        tick_in_bar = abs_tick % bar_ticks
        slot = round(tick_in_bar / slot_ticks)
        slot = min(slot, total_slots - 1)
        if bar_num not in bars_data:
            bars_data[bar_num] = {}
        if instrument not in bars_data[bar_num]:
            bars_data[bar_num][instrument] = {}
        # Keep highest velocity for this slot
        if slot not in bars_data[bar_num][instrument] or velocity > bars_data[bar_num][instrument][slot]:
            bars_data[bar_num][instrument][slot] = velocity

    if not bars_data:
        return "No events to display."

    # Collect all instruments across displayed bars
    all_instruments = sorted({inst for bar in bars_data.values() for inst in bar})

    # Build header
    beat_labels = []
    for beat in range(1, num + 1):
        beat_labels.extend([str(beat), "e", "+", "a"])
    header_str = " ".join(f"{l:>2}" for l in beat_labels)

    lines = []
    for bar_num in sorted(bars_data.keys()):
        lines.append(f"Bar {bar_num}:  {header_str}")
        bar = bars_data[bar_num]
        for inst in all_instruments:
            row = []
            for s in range(total_slots):
                if inst in bar and s in bar[inst]:
                    vel = bar[inst][s]
                    if vel >= 100:
                        row.append(" X")
                    elif vel >= 60:
                        row.append(" x")
                    else:
                        row.append(" o")
                else:
                    row.append(" .")
            label = inst[:12].ljust(12)
            lines.append(f"{label}{''.join(row)}")
        lines.append("")

    return "\n".join(lines)


# ── Session state init ────────────────────────────────────────────────────────

if "history" not in st.session_state:
    st.session_state.history = []
if "arrangement_text" not in st.session_state:
    st.session_state.arrangement_text = ""

config = load_config()

# ── Sidebar ───────────────────────────────────────────────────────────────────

with st.sidebar:
    st.title("\U0001f941 drumgen")

    output_folder = st.text_input("Output Folder", value=config.get("output_folder", DEFAULT_OUTPUT), help="Where .mid files are saved. Use a Windows-accessible path like /mnt/c/... so Ableton can find the files.")
    if output_folder != config.get("output_folder"):
        config["output_folder"] = output_folder
        save_config(config)
    win_path = wsl_to_windows_path(output_folder)
    if win_path != output_folder:
        st.caption(f"Windows: `{win_path}`")

    st.divider()

    # Build cell options first so we can check if a cell is selected before rendering Style
    builtin_cells = sorted(k for k, v in CELLS.items() if v.get("source") != "imported")
    imported_cells_data = sorted(
        [(k, v) for k, v in CELLS.items() if v.get("source") == "imported"],
        key=lambda x: x[0]
    )
    cell_options = ["auto"] + builtin_cells
    imported_labels = {}
    if imported_cells_data:
        cell_options.append("--- imported ---")
        for name, cell_data in imported_cells_data:
            ts = f"{cell_data['time_sig'][0]}/{cell_data['time_sig'][1]}"
            bpm = cell_data.get("source_bpm")
            bpm_str = f" {bpm}bpm" if bpm else ""
            tags = [t for t in cell_data.get("tags", []) if t != "imported"]
            tag_str = f" [{','.join(tags[:3])}]" if tags else ""
            label = f"{name} ({ts}{bpm_str}{tag_str})"
            imported_labels[label] = name
            cell_options.append(label)

    style = st.selectbox(
        "Style", sorted(STYLE_POOLS.keys()),
        disabled=(st.session_state.get("cell_override", "auto") != "auto"),
        help="Genre shortcut — picks cells matching this style. Disabled when a specific cell is selected below.",
    )
    cell_label = st.selectbox("Cell override (auto = let Style choose)", cell_options, key="cell_override", help="Pick a specific rhythmic cell. Overrides Style when not 'auto'. Use this when you know exactly which pattern you want.")
    # Resolve label back to cell name
    cell_name = imported_labels.get(cell_label, cell_label)
    if cell_name == "--- imported ---":
        cell_name = "auto"
    if cell_name != "auto":
        st.caption(f"Style disabled — using cell '{cell_name}' directly.")
    tempo = st.slider("Tempo (BPM)", 40, 300, 120, help="Beats per minute. Blast beats: 160-220. Post-hardcore: 120-150. D-beat: 150-200.")
    if st.session_state.get("use_arrangement", False):
        st.slider("Bars", 1, 32, 4, disabled=True, help="Disabled in arrangement mode — bar count comes from the arrangement string.")
        st.caption("Bars set by arrangement string")
        bars = 4
    else:
        bars = st.slider("Bars", 1, 32, 4, help="Number of bars to generate. Quick loops: 2-4. Full sections: 8-16.")
    time_sig = st.selectbox("Time Signature", ["4/4", "3/4", "6/8", "7/8", "5/4"], help="Most cells are written for 4/4. Odd meter support is limited.")
    humanize = st.slider("Humanize", 0.0, 1.0, 0.7, step=0.05, help="How human the drummer sounds. 0.0 = robotic. 0.2 = tight (Shellac). 0.7 = natural. 0.9+ = barely holding it together.")
    vary = st.slider("Vary", 0.0, 1.0, 0.0, step=0.05, help="How much the pattern mutates on repeats. 0.0 = exact loop. 0.3 = occasional ghost note or kick shift. Keeps longer sections from sounding mechanical.")
    swing = st.slider("Swing", 0.0, 1.0, 0.0, step=0.05, help="Delays upbeat notes. 0.0 = straight. 0.5 = moderate swing. Most underground genres use 0.0.")
    fill_every = st.selectbox("Fill Every N Bars", [0, 2, 4, 8], help="Insert a drum fill every N bars. 0 = no fills. 4 = fill every 4-bar phrase.")

    st.divider()

    use_arrangement = st.checkbox("Use Arrangement Mode", key="use_arrangement", help="Chain multiple sections into one MIDI file (e.g. '4:build 8:drive 2:blast'). Overrides Bars.")

    if use_arrangement:
        st.session_state.arrangement_text = st.text_input(
            "Arrangement",
            value=st.session_state.arrangement_text,
            placeholder="e.g. 4:build 8:drive 2:blast",
        )

        st.caption("Quick-add sections:")
        quick_sections = {
            "intro": 4, "build": 4, "verse": 8, "chorus": 8,
            "drive": 8, "blast": 2, "breakdown": 4, "silence": 1,
            "fill": 1, "atmospheric": 4, "outro": 4,
        }
        cols = st.columns(3)
        for i, (section, default_bars) in enumerate(quick_sections.items()):
            with cols[i % 3]:
                if st.button(f"{default_bars}:{section}", key=f"qa_{section}", use_container_width=True):
                    current = st.session_state.arrangement_text.strip()
                    token = f"{default_bars}:{section}"
                    st.session_state.arrangement_text = f"{current} {token}".strip()
                    st.rerun()
        if st.button("Clear", use_container_width=True):
            st.session_state.arrangement_text = ""
            st.rerun()

    st.divider()

    kit_options = ["ugritone", "general_midi"]
    kit = st.selectbox("Kit", kit_options, help="MIDI note mapping for your drum plugin. Must match your plugin or notes trigger wrong drums.")

    with st.expander("Import MIDI as Cell"):
        uploaded = st.file_uploader("Upload .mid file", type=["mid", "midi"])
        import_name = st.text_input("Cell name", placeholder="auto from filename")
        use_auto_tag = st.checkbox("Auto-detect tags from content", value=True, key="import_auto_tag")
        import_tags = st.text_input("Extra tags (comma-separated)", value="",
                                    help="Added alongside auto-detected tags" if use_auto_tag else "Tags for this cell")
        import_role = st.selectbox("Role", ["groove", "fill", "transition"], key="import_role")

        # Preview auto-tags before importing
        if uploaded and st.button("Preview", key="preview_import"):
            try:
                with tempfile.NamedTemporaryFile(suffix=".mid", delete=False) as tmp:
                    tmp.write(uploaded.getvalue())
                    tmp_path = tmp.name
                cell = midi_to_cell(tmp_path, name=import_name if import_name else None, kit_name=kit, role=import_role)
                if use_auto_tag:
                    auto_tag_cell(cell)
                errors, warnings = validate_cell(cell, kit_name=kit)
                os.unlink(tmp_path)

                ts = f"{cell['time_sig'][0]}/{cell['time_sig'][1]}"
                bpm_str = f" | {cell.get('source_bpm')} BPM" if cell.get('source_bpm') else ""
                st.info(f"**{cell['name']}** | {ts} | {cell['num_bars']} bars | {len(cell['hits'])} hits{bpm_str}")
                st.caption(f"Tags: {', '.join(cell['tags'])}")
                st.caption(f"Role: {cell['role']}")
                if errors:
                    for e in errors:
                        st.warning(f"Validation: {e}")
                if warnings:
                    for w in warnings:
                        st.caption(f"Warning: {w}")
            except Exception as e:
                st.error(f"Preview failed: {e}")

        if st.button("Import") and uploaded:
            try:
                with tempfile.NamedTemporaryFile(suffix=".mid", delete=False) as tmp:
                    tmp.write(uploaded.getvalue())
                    tmp_path = tmp.name
                extra_tags = [t.strip() for t in import_tags.split(",") if t.strip()]
                cell = midi_to_cell(
                    tmp_path,
                    name=import_name if import_name else None,
                    tags=extra_tags if extra_tags else None,
                    kit_name=kit,
                    role=import_role,
                )
                if use_auto_tag:
                    auto_tag_cell(cell)
                    # Merge extra tags
                    if extra_tags:
                        merged = set(cell["tags"]) | set(extra_tags)
                        cell["tags"] = sorted(merged)

                errors, warnings = validate_cell(cell, kit_name=kit)
                if errors:
                    st.warning(f"Validation issues: {'; '.join(errors)}. Importing anyway.")

                path = save_cell(cell)
                os.unlink(tmp_path)
                if path is None:
                    st.warning(f"Skipped **{cell['name']}** — duplicate of existing cell")
                else:
                    bpm_str = f" @ {cell.get('source_bpm')}bpm" if cell.get('source_bpm') else ""
                    st.success(f"Imported **{cell['name']}** ({len(cell['hits'])} hits, {cell['num_bars']} bars{bpm_str})")
                    st.caption(f"Tags: {', '.join(cell['tags'])}")
                    st.caption(f"Saved to `{path}`. Refresh to see in cell dropdown.")
                    st.rerun()
            except Exception as e:
                st.error(f"Import failed: {e}")
                try:
                    os.unlink(tmp_path)
                except OSError:
                    pass

    seed_input = st.number_input("Seed (empty = random)", value=None, min_value=0, step=1, format="%d", help="Random seed for reproducibility. Same seed + same settings = same pattern. Leave empty for random.")
    seed = int(seed_input) if seed_input is not None else None

    default_fn = auto_filename(style, tempo, bars, arrangement=use_arrangement)
    filename = st.text_input("Filename", value=default_fn)

    generate = st.button("Generate", type="primary", use_container_width=True)

# ── Main area ─────────────────────────────────────────────────────────────────

st.header("drumgen")

if generate:
    try:
        if use_arrangement:
            arr_text = st.session_state.arrangement_text.strip()
            if not arr_text:
                st.error("Arrangement text is empty. Add sections or type an arrangement string.")
                st.stop()
            result = assemble_arrangement(
                style=style,
                arrangement_str=arr_text,
                tempo=tempo,
                time_sig=time_sig,
                humanize=humanize,
                swing=swing,
                seed=seed,
                vary=vary,
            )
            total_bars = result["total_bars"]
        else:
            resolved_cell = cell_name if cell_name != "auto" else None
            result = assemble(
                style=style,
                cell_name=resolved_cell,
                bars=bars,
                tempo=tempo,
                time_sig=time_sig,
                humanize=humanize,
                swing=swing,
                fill_every=fill_every,
                seed=seed,
                vary=vary,
            )
            total_bars = bars
            # Track which cell was actually used
            if resolved_cell:
                actual_cell = resolved_cell
            else:
                actual_cell = STYLE_MAP.get(style.lower(), "?")
            result["actual_cell"] = actual_cell

        output_path = os.path.join(output_folder, filename)
        write_midi(
            events=result["events"],
            tempo=result["tempo"],
            time_signatures=result["time_signatures"],
            kit_mapping_path=kit,
            output_path=output_path,
        )

        # Read file bytes for download
        with open(output_path, "rb") as f:
            midi_bytes = f.read()

        # Store in session state
        st.session_state.last_result = {
            "events": result["events"],
            "time_signatures": result["time_signatures"],
            "tempo": result["tempo"],
            "seed": result["seed"],
            "total_bars": total_bars,
            "filename": filename,
            "output_path": output_path,
            "midi_bytes": midi_bytes,
            "style": style,
            "humanize": humanize,
            "cell": cell_name,
            "actual_cell": result.get("actual_cell", ""),
            "arrangement": use_arrangement,
            "section_summary": result.get("section_summary", ""),
        }

        # Add to history
        st.session_state.history.insert(0, {
            "time": datetime.now().strftime("%H:%M:%S"),
            "filename": filename,
            "style": style,
            "tempo": tempo,
            "bars": total_bars,
            "seed": result["seed"],
        })
        st.session_state.history = st.session_state.history[:10]

    except Exception as e:
        st.error(f"Generation failed: {e}")

# ── Display results ───────────────────────────────────────────────────────────

if "last_result" in st.session_state:
    r = st.session_state.last_result
    win_out = wsl_to_windows_path(r["output_path"])

    st.success(f"Saved: **{r['filename']}**")
    st.caption(f"WSL: `{r['output_path']}`")
    if win_out != r["output_path"]:
        st.caption(f"Windows: `{win_out}`")

    if r.get("actual_cell") and not r["arrangement"]:
        if r["cell"] != "auto":
            st.info(f"Cell used: **{r['actual_cell']}** (manually selected)")
        else:
            st.info(f"Cell used: **{r['actual_cell']}** (from style '{r['style']}')")
        cell_data = CELLS.get(r["actual_cell"])
        if cell_data and cell_data.get("tags"):
            st.caption(f"Tags: {', '.join(cell_data['tags'])}")

    col1, col2 = st.columns([1, 1])

    with col1:
        st.subheader("Parameters")
        params = {
            "Style": r["style"],
            "Tempo": f"{r['tempo']} BPM",
            "Bars": r["total_bars"],
            "Cell": r["cell"],
            "Humanize": r["humanize"],
            "Seed": r["seed"],
        }
        if r["arrangement"] and r.get("section_summary"):
            params["Sections"] = r["section_summary"]
        for k, v in params.items():
            st.text(f"{k}: {v}")

    with col2:
        st.subheader("Stats")
        events = r["events"]
        total_hits = len(events)
        unique_instruments = len({inst for _, inst, _ in events})
        velocities = [v for _, _, v in events]
        vel_min = min(velocities) if velocities else 0
        vel_max = max(velocities) if velocities else 0
        st.text(f"Total hits: {total_hits}")
        st.text(f"Unique instruments: {unique_instruments}")
        st.text(f"Velocity range: {vel_min}–{vel_max}")

    st.download_button(
        label="Download .mid",
        data=r["midi_bytes"],
        file_name=r["filename"],
        mime="audio/midi",
        use_container_width=True,
    )

    # ── Audio preview ─────────────────────────────────────────────────────
    st.subheader("Audio Preview")
    if not is_fluidsynth_available():
        st.caption("Audio preview unavailable — install FluidSynth: `sudo zypper install fluidsynth fluid-soundfont-gm`")
    elif find_soundfont() is None:
        st.caption("Audio preview unavailable — no soundfont found. Run: `sudo zypper install fluid-soundfont-gm`")
    else:
        # Render to temp wav if not already cached for this result
        if "wav_bytes" not in r:
            with tempfile.NamedTemporaryFile(suffix=".wav", delete=False) as tmp:
                tmp_wav = tmp.name
            with st.spinner("Rendering preview..."):
                wav_result = render_midi_to_wav(r["output_path"], tmp_wav)
            if wav_result:
                with open(tmp_wav, "rb") as f:
                    r["wav_bytes"] = f.read()
                os.unlink(tmp_wav)
            else:
                r["wav_bytes"] = None
                try:
                    os.unlink(tmp_wav)
                except OSError:
                    pass

        if r.get("wav_bytes"):
            st.audio(r["wav_bytes"], format="audio/wav")
            st.caption("Preview uses General MIDI drums — your Ugritone kit will sound different.")
        else:
            st.caption("Preview rendering failed.")

    st.subheader("Pattern Preview")
    st.caption("**X** = accent (loud)  **x** = normal  **o** = ghost (quiet)  **.** = silent")
    preview = build_pattern_preview(r["events"], r["time_signatures"], r["tempo"])
    st.code(preview, language=None)

# ── Session history ───────────────────────────────────────────────────────────

if st.session_state.history:
    st.divider()
    st.subheader("History")
    for entry in st.session_state.history:
        st.text(f"[{entry['time']}] {entry['filename']} — {entry['style']} @ {entry['tempo']}bpm, {entry['bars']} bars (seed {entry['seed']})")
