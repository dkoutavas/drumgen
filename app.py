import streamlit as st
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path
from datetime import datetime

from cell_library import CELLS, STYLE_POOLS, STYLE_MAP, SECTION_PREFERENCES, list_cells
from assembler import assemble, assemble_arrangement, assemble_layered
from midi_engine import write_midi, DEFAULT_PPQ, unique_filepath
from preview import render_midi_to_wav, is_fluidsynth_available, find_soundfont
from midi_reader import midi_to_cell, save_cell, auto_tag_cell, validate_cell

st.set_page_config(page_title="drumgen", page_icon="\U0001f941", layout="wide", initial_sidebar_state="expanded")

# ── Persistent config ─────────────────────────────────────────────────────────

CONFIG_PATH = Path(__file__).parent / ".drumgen_config.json"


def _detect_default_output():
    """Pick a sensible default output folder based on platform."""
    if sys.platform == "win32":
        return str(Path.home() / "Documents" / "drumgen_output")
    # WSL: default to Windows Documents so Ableton can access files directly
    if Path("/mnt/c").is_dir():
        try:
            result = subprocess.run(
                ["cmd.exe", "/C", "echo %USERNAME%"],
                capture_output=True, text=True, timeout=5,
            )
            win_user = result.stdout.strip()
            if win_user:
                wsl_path = f"/mnt/c/Users/{win_user}/Documents/drumgen_output"
                return wsl_path
        except (FileNotFoundError, subprocess.TimeoutExpired):
            pass
    # macOS / native Linux fallback
    return str(Path(__file__).parent / "output")


DEFAULT_OUTPUT = _detect_default_output()


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


def windows_to_wsl(win_path):
    win_path = win_path.replace("\\", "/")
    if len(win_path) >= 2 and win_path[1] == ":":
        drive = win_path[0].lower()
        return f"/mnt/{drive}/{win_path[3:]}"
    return win_path


def _open_folder(path):
    """Open a folder in the platform's file manager."""
    if sys.platform == "win32":
        os.startfile(path)
    else:
        # Use wslpath for reliable conversion (handles both /mnt/ and native WSL paths)
        try:
            result = subprocess.run(
                ["wslpath", "-w", path], capture_output=True, text=True, timeout=5
            )
            win_path = result.stdout.strip() if result.returncode == 0 else wsl_to_windows_path(path)
        except (FileNotFoundError, subprocess.TimeoutExpired):
            win_path = wsl_to_windows_path(path)
        subprocess.Popen(["explorer.exe", win_path])


# ── Auto filename ─────────────────────────────────────────────────────────────

def auto_filename(style, tempo, bars, arrangement=False, cell_name="auto",
                  layer_active=False, layers=None, generative=False, time_sig="4/4"):
    parts = []
    if generative:
        parts.append("gen")
    if layer_active and layers:
        active = [f"{k[0]}_{v[:15]}" for k, v in layers.items() if v != "none"]
        parts.append("layer_" + "_".join(active) if active else "layer")
    elif cell_name != "auto":
        parts.append(cell_name)
    else:
        parts.append(style)
    parts.append(f"{tempo}bpm")
    if time_sig != "4/4":
        parts.append(time_sig.replace("/", "_"))
    parts.append("arrangement" if arrangement else f"{bars}bars")
    return "_".join(parts) + ".mid"


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

# ── Pre-compute mode state (for disabling widgets rendered before mode widgets) ──

_layer_active = any(st.session_state.get(k, "none") != "none"
                    for k in ["layer_kick", "layer_snare", "layer_cymbal", "layer_toms"])
_use_arrangement = st.session_state.get("use_arrangement", False)

# ── Build cell options (needed before sidebar renders) ────────────────────────

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

# ── Sidebar ───────────────────────────────────────────────────────────────────

with st.sidebar:
    st.title("\U0001f941 drumgen")

    # ── Output ────────────────────────────────────────────────────────────
    st.markdown("##### Output")
    stored_wsl = config.get("output_folder", DEFAULT_OUTPUT)
    initial_win = wsl_to_windows_path(stored_wsl)
    win_input = st.text_input(
        "Output Folder", value=initial_win, key="output_folder_win",
        help="Where .mid files are saved. Accepts Windows or WSL paths.",
    )
    output_folder = windows_to_wsl(win_input)
    if output_folder != config.get("output_folder"):
        config["output_folder"] = output_folder
        save_config(config)
    if sys.platform == "win32":
        st.caption(f"Path: `{output_folder}`")
    elif Path("/mnt/c").is_dir():
        try:
            r = subprocess.run(["wslpath", "-w", output_folder], capture_output=True, text=True, timeout=5)
            win_display = r.stdout.strip() if r.returncode == 0 else wsl_to_windows_path(output_folder)
        except (FileNotFoundError, subprocess.TimeoutExpired):
            win_display = wsl_to_windows_path(output_folder)
        st.caption(f"WSL: `{win_display}`")
    else:
        st.caption(f"Path: `{output_folder}`")
    if st.button("Open folder", use_container_width=True, key="open_folder_sidebar"):
        _open_folder(output_folder)

    kit_options = ["ugritone", "general_midi"]
    kit = st.selectbox("Kit", kit_options, help="MIDI note mapping for your drum plugin.")
    st.divider()

    # ── What ──────────────────────────────────────────────────────────────
    st.markdown("##### What")
    _cell_override = st.session_state.get("cell_override", "auto")
    style = st.selectbox(
        "Style", sorted(STYLE_POOLS.keys()),
        disabled=(_layer_active or _cell_override != "auto"),
        help="Genre shortcut — picks cells matching this style.",
    )
    cell_label = st.selectbox(
        "Cell override (auto = let Style choose)", cell_options,
        key="cell_override", disabled=_layer_active,
        help="Pick a specific rhythmic cell. Overrides Style when not 'auto'.",
    )
    cell_name = imported_labels.get(cell_label, cell_label)
    if cell_name == "--- imported ---":
        cell_name = "auto"
    if _layer_active:
        st.caption("Style & Cell disabled \u2014 layer mode active")
    elif cell_name != "auto":
        st.caption(f"Style disabled \u2014 using cell '{cell_name}' directly")

    generative = st.checkbox("Generative mode", value=False,
        help="Probability-based generation. Same style, different each time.")
    num_variations = 1
    if generative:
        num_variations = st.number_input("Variations", min_value=1, max_value=20, value=1)
    st.divider()

    # ── Sound ─────────────────────────────────────────────────────────────
    st.markdown("##### Sound")
    tempo = st.slider("Tempo (BPM)", 40, 300, 120,
        help="Beats per minute. Blast beats: 160-220. Post-hardcore: 120-150.")
    time_sig = st.selectbox("Time Signature", ["4/4", "3/4", "6/4", "6/8", "7/8", "5/4"],
        help="Most cells are written for 4/4. Odd meter support is limited.")
    if _use_arrangement:
        st.slider("Bars", 1, 32, 4, disabled=True,
            help="Disabled in arrangement mode \u2014 bar count comes from the arrangement string.")
        st.caption("Bars set by arrangement string")
        bars = 4
    else:
        bars = st.slider("Bars", 1, 32, 4,
            help="Number of bars to generate. Quick loops: 2-4. Full sections: 8-16.")
    st.divider()

    # ── Feel ──────────────────────────────────────────────────────────────
    st.markdown("##### Feel")
    humanize = st.slider("Humanize", 0.0, 1.0, 0.7, step=0.05,
        help="How human the drummer sounds. 0.0 = robotic. 0.7 = natural.")
    swing = st.slider("Swing", 0.0, 1.0, 0.0, step=0.05,
        help="Delays upbeat notes. 0.0 = straight. 0.5 = moderate swing.")
    vary = st.slider("Vary", 0.0, 1.0, 0.0, step=0.05,
        help="How much the pattern mutates on repeats. 0.0 = exact loop.")
    fill_every = st.selectbox("Fill Every N Bars", [0, 2, 4, 8],
        help="Insert a drum fill every N bars. 0 = no fills.")
    st.divider()

    # ── Modes ─────────────────────────────────────────────────────────────
    st.markdown("##### Modes")
    with st.expander("Layer Mode (mix cells)"):
        st.caption("Pick cells per instrument group. Overrides Style/Cell.")
        sorted_cell_names = sorted(CELLS.keys())
        layer_kick = st.selectbox("Kick", ["none"] + sorted_cell_names, key="layer_kick")
        layer_snare = st.selectbox("Snare", ["none"] + sorted_cell_names, key="layer_snare")
        layer_cymbal = st.selectbox("Cymbal", ["none"] + sorted_cell_names, key="layer_cymbal")
        layer_toms = st.selectbox("Toms", ["none"] + sorted_cell_names, key="layer_toms")
        if st.button("Clear all layers", use_container_width=True):
            st.session_state.layer_kick = "none"
            st.session_state.layer_snare = "none"
            st.session_state.layer_cymbal = "none"
            st.session_state.layer_toms = "none"
            st.rerun()

    layers_map = {"kick": layer_kick, "snare": layer_snare, "cymbal": layer_cymbal, "toms": layer_toms}
    layer_active = any(v != "none" for v in layers_map.values())
    if layer_active:
        active_summary = " | ".join(f"{k[0].upper()}:{v}" for k, v in layers_map.items() if v != "none")
        st.caption(f"Layers: {active_summary}")

    if layer_active:
        cell_name = "auto"

    use_arrangement = st.checkbox("Use Arrangement Mode", key="use_arrangement",
        help="Chain multiple sections into one MIDI file.")

    if use_arrangement:
        st.session_state.arrangement_text = st.text_input(
            "Arrangement",
            value=st.session_state.arrangement_text,
            placeholder="4:build 8:drive 2:blast 4:breakdown 1:fill 4:outro",
            help="Format: BARS:SECTION_TYPE. Example: '4:build 8:drive 2:blast'",
        )
        st.caption("Syntax: `N:section` — sections: intro, build, verse, chorus, drive, blast, breakdown, atmospheric, silence, fill, outro. Append `@N/M` for odd meters (e.g. `4:verse@7/8`).")
        quick_meter = st.selectbox("Meter for quick-add",
            ["(use global)", "4/4", "3/4", "6/4", "6/8", "7/8", "5/4"], key="quick_meter")

        st.caption("Quick-add sections:")
        quick_sections = {
            "intro": 4, "build": 4, "verse": 8, "chorus": 8,
            "drive": 8, "blast": 2, "breakdown": 4, "silence": 1,
            "fill": 1, "atmospheric": 4, "outro": 4,
        }
        cols = st.columns(3)
        for i, (section, default_bars) in enumerate(quick_sections.items()):
            with cols[i % 3]:
                meter_suffix = f"@{quick_meter}" if quick_meter != "(use global)" else ""
                btn_label = f"{default_bars}:{section}{meter_suffix}"
                if st.button(btn_label, key=f"qa_{section}", use_container_width=True):
                    current = st.session_state.arrangement_text.strip()
                    token = f"{default_bars}:{section}{meter_suffix}"
                    st.session_state.arrangement_text = f"{current} {token}".strip()
                    st.rerun()
        if st.button("Clear", use_container_width=True, key="clear_arrangement"):
            st.session_state.arrangement_text = ""
            st.rerun()
    st.divider()

    # ── Generate ──────────────────────────────────────────────────────────
    st.markdown("##### Generate")
    seed_input = st.number_input("Seed (empty = random)", value=None, min_value=0, step=1, format="%d",
        help="Random seed for reproducibility. Same seed + same settings = same pattern.")
    seed = int(seed_input) if seed_input is not None else None

    default_fn = auto_filename(
        style, tempo, bars,
        arrangement=use_arrangement,
        cell_name=cell_name,
        layer_active=layer_active,
        layers=layers_map,
        generative=generative,
        time_sig=time_sig,
    )
    if "prev_auto_fn" not in st.session_state:
        st.session_state.prev_auto_fn = ""
    if default_fn != st.session_state.prev_auto_fn:
        st.session_state.filename_input = default_fn
        st.session_state.prev_auto_fn = default_fn
    filename = st.text_input("Filename", key="filename_input")

    auto_play = st.checkbox("Auto-play audio after generate", value=False, key="auto_play")

    generate = st.button("Generate", type="primary", use_container_width=True)

    with st.expander("Import MIDI as Cell"):
        uploaded = st.file_uploader("Upload .mid file", type=["mid", "midi"])
        import_name = st.text_input("Cell name", placeholder="auto from filename")
        use_auto_tag = st.checkbox("Auto-detect tags from content", value=True, key="import_auto_tag")
        import_tags = st.text_input("Extra tags (comma-separated)", value="",
                                    help="Added alongside auto-detected tags" if use_auto_tag else "Tags for this cell")
        import_role = st.selectbox("Role", ["groove", "fill", "transition"], key="import_role")

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

                ts_str = f"{cell['time_sig'][0]}/{cell['time_sig'][1]}"
                bpm_str = f" | {cell.get('source_bpm')} BPM" if cell.get('source_bpm') else ""
                st.info(f"**{cell['name']}** | {ts_str} | {cell['num_bars']} bars | {len(cell['hits'])} hits{bpm_str}")
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
                    if extra_tags:
                        merged = set(cell["tags"]) | set(extra_tags)
                        cell["tags"] = sorted(merged)

                errors, warnings = validate_cell(cell, kit_name=kit)
                if errors:
                    st.warning(f"Validation issues: {'; '.join(errors)}. Importing anyway.")

                path = save_cell(cell)
                os.unlink(tmp_path)
                if path is None:
                    st.warning(f"Skipped **{cell['name']}** \u2014 duplicate of existing cell")
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

# ── Main area ─────────────────────────────────────────────────────────────────

st.header("drumgen")
generate_top = st.button("Generate", type="primary", key="generate_top")

if generate or generate_top:
    try:
        variation_files = []
        resolved_cell = None

        if layer_active:
            # Layer mode
            layers = {}
            if layer_kick != "none":
                layers["kick"] = layer_kick
            if layer_snare != "none":
                layers["snare"] = layer_snare
            if layer_cymbal != "none":
                layers["cymbal"] = layer_cymbal
            if layer_toms != "none":
                layers["toms"] = layer_toms
            result = assemble_layered(
                layers=layers,
                bars=bars,
                tempo=tempo,
                time_sig=time_sig,
                humanize=humanize,
                swing=swing,
                vary=vary,
                seed=seed,
            )
            total_bars = bars
            result["actual_cell"] = "layered: " + ", ".join(f"{k}={v}" for k, v in layers.items())
        elif use_arrangement:
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
                generative=generative,
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
                generative=generative,
            )
            total_bars = bars
            if resolved_cell:
                actual_cell = resolved_cell
            else:
                actual_cell = STYLE_MAP.get(style.lower(), "?")
            result["actual_cell"] = actual_cell

        # Handle variations
        if generative and num_variations > 1 and not layer_active:
            base_seed = result["seed"]
            variation_results = [result]
            for vi in range(1, num_variations):
                var_seed = base_seed + vi
                if use_arrangement:
                    vr = assemble_arrangement(
                        style=style, arrangement_str=st.session_state.arrangement_text.strip(),
                        tempo=tempo, time_sig=time_sig, humanize=humanize,
                        swing=swing, seed=var_seed, vary=vary, generative=True,
                    )
                else:
                    vr = assemble(
                        style=style, cell_name=resolved_cell,
                        bars=bars, tempo=tempo, time_sig=time_sig, humanize=humanize,
                        swing=swing, fill_every=fill_every, seed=var_seed, vary=vary,
                        generative=True,
                    )
                variation_results.append(vr)

            base_name, ext = os.path.splitext(filename)
            for vi, vr in enumerate(variation_results, 1):
                var_path = os.path.join(output_folder, f"{base_name}_v{vi}{ext}")
                var_path = unique_filepath(var_path)
                var_filename = os.path.basename(var_path)
                write_midi(
                    events=vr["events"], tempo=vr["tempo"],
                    time_signatures=vr["time_signatures"],
                    kit_mapping_path=kit, output_path=var_path,
                )
                with open(var_path, "rb") as f:
                    var_bytes = f.read()
                variation_files.append({
                    "filename": var_filename, "seed": vr["seed"], "bytes": var_bytes,
                })
            result = variation_results[0]

        output_path = os.path.join(output_folder, filename)
        output_path = unique_filepath(output_path)
        filename = os.path.basename(output_path)
        write_midi(
            events=result["events"],
            tempo=result["tempo"],
            time_signatures=result["time_signatures"],
            kit_mapping_path=kit,
            output_path=output_path,
        )

        with open(output_path, "rb") as f:
            midi_bytes = f.read()

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
            "variation_files": variation_files,
        }

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

    # Row 1: Success + Download + Open folder
    col_msg, col_dl, col_open = st.columns([3, 1, 1])
    with col_msg:
        st.success(f"Saved: **{r['filename']}**")
    with col_dl:
        st.download_button(
            label="Download .mid",
            data=r["midi_bytes"],
            file_name=r["filename"],
            mime="audio/midi",
            use_container_width=True,
        )
    with col_open:
        if st.button("Open folder", use_container_width=True, key="open_folder_result"):
            _open_folder(os.path.dirname(r["output_path"]))

    # Row 2: Compact info caption
    cell_info = ""
    if r.get("actual_cell") and not r["arrangement"]:
        cell_info = f"Cell: {r['actual_cell']} | "
    st.caption(f"{cell_info}Seed: {r['seed']} | Path: `{win_out}`")

    # Variation downloads
    if r.get("variation_files"):
        vcols = st.columns(min(len(r["variation_files"]), 5))
        for i, vf in enumerate(r["variation_files"]):
            with vcols[i % 5]:
                st.download_button(
                    label=f"v{i+1} (seed {vf['seed']})",
                    data=vf["bytes"],
                    file_name=vf["filename"],
                    mime="audio/midi",
                    use_container_width=True,
                    key=f"dl_var_{i+1}",
                )

    # Row 3: Audio preview
    if not is_fluidsynth_available():
        st.caption("Audio preview unavailable — install [FluidSynth](https://github.com/FluidSynth/fluidsynth/wiki/Download): "
                   "`apt install fluidsynth fluid-soundfont-gm` (Debian/Ubuntu) · "
                   "`brew install fluid-synth` (macOS) · "
                   "`zypper install fluidsynth fluid-soundfont-gm` (openSUSE)")
    elif find_soundfont() is None:
        st.caption("Audio preview unavailable — FluidSynth found but no soundfont. "
                   "Install a GM soundfont: `apt install fluid-soundfont-gm` (Debian/Ubuntu) · "
                   "`zypper install fluid-soundfont-gm` (openSUSE)")
    else:
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
            st.audio(r["wav_bytes"], format="audio/wav",
                     autoplay=st.session_state.get("auto_play", False))
            st.caption("Preview uses General MIDI drums \u2014 your Ugritone kit will sound different.")
        else:
            st.caption("Preview rendering failed.")

    # Row 4: Pattern Preview (expanded by default)
    with st.expander("Pattern Preview", expanded=True):
        st.caption("**X** = accent (loud)  **x** = normal  **o** = ghost (quiet)  **.** = silent")
        preview = build_pattern_preview(r["events"], r["time_signatures"], r["tempo"])
        st.code(preview, language=None)

    # Row 5: Parameters & Stats (collapsed by default)
    with st.expander("Parameters & Stats"):
        col1, col2 = st.columns([1, 1])
        with col1:
            st.markdown("**Parameters**")
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
            st.markdown("**Stats**")
            events = r["events"]
            total_hits = len(events)
            unique_instruments = len({inst for _, inst, _ in events})
            velocities = [v for _, _, v in events]
            vel_min = min(velocities) if velocities else 0
            vel_max = max(velocities) if velocities else 0
            st.text(f"Total hits: {total_hits}")
            st.text(f"Unique instruments: {unique_instruments}")
            st.text(f"Velocity range: {vel_min}\u2013{vel_max}")

# ── Session history ───────────────────────────────────────────────────────────

if st.session_state.history:
    st.divider()
    st.subheader("History")
    for entry in st.session_state.history:
        st.text(f"[{entry['time']}] {entry['filename']} \u2014 {entry['style']} @ {entry['tempo']}bpm, {entry['bars']} bars (seed {entry['seed']})")
