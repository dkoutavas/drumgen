"""Where output goes and how to open it, per platform.

Native Linux is the target. The WSL and Windows branches exist for the Python
CLI/GUI only (the plugin has its own paths) and are kept deliberately small.
"""
import os
import subprocess
import sys
from pathlib import Path

REPO_OUTPUT = Path(__file__).parent / "output"


def is_wsl():
    """True only under WSL. `/mnt/c` existing is NOT enough: a native Linux
    box can have one, and the old check then spawned cmd.exe at import time."""
    if sys.platform != "linux":
        return False
    try:
        return "microsoft" in Path("/proc/version").read_text().lower()
    except OSError:
        return False


def default_output_dir():
    """Windows Documents on Windows and WSL (so a Windows DAW can read the
    files directly), `output/` next to the code everywhere else."""
    if sys.platform == "win32":
        return str(Path.home() / "Documents" / "drumgen_output")
    if is_wsl():
        try:
            result = subprocess.run(
                ["cmd.exe", "/C", "echo %USERNAME%"],
                capture_output=True, text=True, timeout=5,
            )
            win_user = result.stdout.strip()
            if win_user:
                return f"/mnt/c/Users/{win_user}/Documents/drumgen_output"
        except (FileNotFoundError, subprocess.TimeoutExpired):
            pass
    return str(REPO_OUTPUT)


def _wsl_to_windows_path(wsl_path):
    if wsl_path.startswith("/mnt/") and len(wsl_path) > 6 and wsl_path[5].isalpha() and wsl_path[6] == "/":
        return f"{wsl_path[5].upper()}:\\{wsl_path[7:].replace('/', chr(92))}"
    return wsl_path


def open_folder(path):
    """Open a folder in the platform's file manager. Returns the command used
    (or None on Windows, which uses `os.startfile`), so it can be tested."""
    if sys.platform == "win32":
        os.startfile(path)  # type: ignore[attr-defined]
        return None
    if is_wsl():
        try:
            out = subprocess.run(["wslpath", "-w", path], capture_output=True, text=True, timeout=5)
            target = out.stdout.strip() if out.returncode == 0 else _wsl_to_windows_path(path)
        except (FileNotFoundError, subprocess.TimeoutExpired):
            target = _wsl_to_windows_path(path)
        cmd = ["explorer.exe", target]
    elif sys.platform == "darwin":
        cmd = ["open", path]
    else:
        cmd = ["xdg-open", path]
    try:
        subprocess.Popen(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    except FileNotFoundError:
        return None  # no file manager launcher installed: nothing to open
    return cmd
