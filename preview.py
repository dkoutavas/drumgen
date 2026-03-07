import subprocess
import shutil
import wave
from pathlib import Path

import mido

SOUNDFONT_PATHS = [
    Path(__file__).parent / "soundfonts" / "FluidR3_GM.sf2",
    Path("/usr/share/sounds/sf2/FluidR3_GM.sf2"),
    Path("/usr/share/soundfonts/FluidR3_GM.sf2"),
    Path("/usr/share/sounds/sf2/FluidR3_GM2-2.sf2"),
]


def find_soundfont():
    for path in SOUNDFONT_PATHS:
        if path.exists():
            return str(path)
    return None


def is_fluidsynth_available():
    return shutil.which("fluidsynth") is not None


def _trim_wav_to_midi(midi_path, wav_path, tail=0.5):
    """Trim a rendered wav to the MIDI duration plus a short decay tail."""
    mid = mido.MidiFile(midi_path)
    target_sec = mid.length + tail
    with wave.open(wav_path, 'rb') as r:
        framerate = r.getframerate()
        n_channels = r.getnchannels()
        sampwidth = r.getsampwidth()
        target_frames = min(int(target_sec * framerate), r.getnframes())
        frames = r.readframes(target_frames)
    with wave.open(wav_path, 'wb') as w:
        w.setnchannels(n_channels)
        w.setsampwidth(sampwidth)
        w.setframerate(framerate)
        w.writeframes(frames)


def render_midi_to_wav(midi_path, wav_path=None, soundfont_path=None):
    if not is_fluidsynth_available():
        return None

    if soundfont_path is None:
        soundfont_path = find_soundfont()

    if soundfont_path is None:
        return None

    if wav_path is None:
        wav_path = str(Path(midi_path).with_suffix(".wav"))

    try:
        result = subprocess.run(
            [
                "fluidsynth",
                "-ni",
                "-F", str(wav_path),
                "-r", "44100",
                "-g", "1.0",
                soundfont_path,
                str(midi_path),
            ],
            capture_output=True,
            text=True,
            timeout=30,
        )
        if result.returncode == 0 and Path(wav_path).exists():
            _trim_wav_to_midi(str(midi_path), wav_path)
            return wav_path
        return None
    except (subprocess.TimeoutExpired, FileNotFoundError):
        return None
