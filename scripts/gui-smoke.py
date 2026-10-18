#!/usr/bin/env python3
"""Headless smoke test for the Streamlit GUI (app.py).

Nothing in the pytest suite imports app.py (Streamlit is heavy and not a test
dependency), so this drives the real page in a browser instead: loop mode, a
forced 7/8, arrangement mode, layer mode and the Open folder button. It fails
on any `st.error`, any "Traceback" on the page, or a missing "Saved:" message.

Not run in CI. Run it locally when app.py changes:

    pip install playwright             # once; uses an installed Chromium
    streamlit run app.py --server.headless true --server.port 8765 &
    python scripts/gui-smoke.py --url http://localhost:8765 --shots /tmp/shots

`--chromium PATH` points at a browser binary when Playwright's own download
is not available (cloud sessions ship one at /opt/pw-browsers/chromium).
"""
import argparse
import os
import re
import sys
import time

from playwright.sync_api import sync_playwright

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
from cell_library import CELLS  # noqa: E402  (to check the reported cell's meter)

TIMEOUT = 30_000


def wait_idle(page):
    """Streamlit reruns the script on every interaction; wait until it settles."""
    page.wait_for_load_state("networkidle")
    page.locator('[data-testid="stStatusWidget"]').wait_for(state="hidden", timeout=TIMEOUT)


def select(page, label, option):
    """Pick `option` in the selectbox whose label is `label`."""
    box = page.locator('[data-testid="stSelectbox"]').filter(has_text=label).first
    box.locator('[role="combobox"]').click()
    # Long option lists are virtualized: type to filter so the option exists.
    page.keyboard.type(option)
    page.get_by_role("option", name=option, exact=True).first.click()
    wait_idle(page)


def text_now(locator):
    """Text of the element if it is on the page right now, else None.
    Elements detach while Streamlit reruns; never block on one."""
    try:
        return locator.inner_text(timeout=250)
    except Exception:
        return None


def generate(page):
    """Click Generate and wait for THIS run's result. After the first run a
    "Saved:" line is already on the page, so waiting for it alone returns
    before the rerun finishes; wait until the saved filename changes."""
    saved = page.get_by_text("Saved:").first
    preview = page.locator('[data-testid="stCode"]').first
    before = text_now(saved)
    before_preview = text_now(preview)
    page.locator(".st-key-generate_top button").click()
    # Streamlit streams a rerun top to bottom: the "Saved:" line updates before
    # the preview further down, so wait for BOTH to show this run.
    deadline = time.monotonic() + TIMEOUT / 1000
    while time.monotonic() < deadline:
        now_saved, now_preview = text_now(saved), text_now(preview)
        saved_new = now_saved is not None and now_saved != before
        preview_new = now_preview is not None and now_preview != before_preview
        if saved_new and preview_new:
            break
        page.wait_for_timeout(100)
    else:
        raise AssertionError("Generate did not produce a new result")
    wait_idle(page)


def assert_clean(page, step):
    body = page.locator("body").inner_text()
    for bad in ("Traceback", "Generation failed", "Error:"):
        if bad in body:
            raise AssertionError(f"{step}: page shows {bad!r}:\n{body[:2000]}")


def caption(page):
    """The 'Cell: ... | Seed: ...' line under a successful generation."""
    loc = page.get_by_text("Seed:").first
    return loc.inner_text() if loc.count() else ""


def reported_cell(page):
    m = re.search(r"Cell: ([^|]+?) \|", caption(page))
    return m.group(1).strip() if m else None


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--url", default="http://localhost:8765")
    ap.add_argument("--shots", default=".", help="directory for screenshots")
    ap.add_argument("--chromium", default=None, help="browser executable to use")
    args = ap.parse_args()
    os.makedirs(args.shots, exist_ok=True)
    shot = lambda page, name: page.screenshot(path=os.path.join(args.shots, name), full_page=True)

    results = []
    with sync_playwright() as p:
        browser = p.chromium.launch(executable_path=args.chromium) if args.chromium else p.chromium.launch()
        page = browser.new_page(viewport={"width": 1400, "height": 1100})
        page.set_default_timeout(TIMEOUT)
        page.goto(args.url)
        page.get_by_text("Time Signature").first.wait_for()
        wait_idle(page)

        # 1. Loop mode with the defaults.
        generate(page)
        assert_clean(page, "loop mode")
        results.append(("loop mode", caption(page)))
        shot(page, "1-loop.png")

        # 2. Forced 7/8 on a style that OWNS 7/8 cells: the caption must name
        # the cell that played (a 7/8 one), not the style's first pool cell
        # (the bug fixed 2026-10-02). The preview must draw 14 columns per bar.
        select(page, "Style", "faraquet")
        select(page, "Time Signature", "7/8")
        generate(page)
        assert_clean(page, "7/8")
        cell = reported_cell(page)
        results.append(("7/8", caption(page)))
        if cell not in CELLS or tuple(CELLS[cell]["time_sig"]) != (7, 8):
            raise AssertionError(f"7/8 generation reports a non-7/8 cell: {cell!r}")
        preview = page.locator('[data-testid="stCode"]').first.inner_text()
        first = preview.splitlines()[0] if preview else ""
        if first.split(":", 1)[-1].split() != "1 + 2 + 3 + 4 + 5 + 6 + 7 +".split():
            raise AssertionError(f"7/8 preview is not one column per sixteenth: {first!r}")
        shot(page, "2-seven-eight.png")
        select(page, "Time Signature", "4/4")

        # 3. Open folder: on a box without a file manager it must not crash.
        btn = page.locator(".st-key-open_folder_result button")
        if btn.count():
            btn.click()
            wait_idle(page)
            assert_clean(page, "open folder")
            results.append(("open folder", "no crash"))

        # 4. Arrangement mode, built with the quick-add buttons: 2 bars of
        # blast, then a verse in 7/8. Bars 3-4 of the preview must be drawn in
        # 7/8 (the preview used to bucket every bar by the first meter).
        page.locator(".st-key-use_arrangement label").click()
        wait_idle(page)
        page.locator(".st-key-qa_blast button").click()
        wait_idle(page)
        select(page, "Meter for quick-add", "7/8")
        page.locator(".st-key-qa_verse button").click()
        wait_idle(page)
        generate(page)
        assert_clean(page, "arrangement")
        preview = page.locator('[data-testid="stCode"]').first.inner_text()
        results.append(("arrangement", "2:blast 8:verse@7/8"))
        if "Bar 3 (7/8):" not in preview:
            raise AssertionError(f"mixed-meter preview did not turn to 7/8 at bar 3:\n{preview[:600]}")
        shot(page, "3-arrangement.png")
        page.locator(".st-key-clear_arrangement button").click()
        wait_idle(page)
        page.locator(".st-key-use_arrangement label").click()
        wait_idle(page)

        # 5. Layer mode: a Euclidean toms layer (phasing was restored 2026-10-02).
        toms = page.locator(".st-key-layer_toms [role='combobox']")
        if not toms.is_visible():  # the expander toggles; open it only if shut
            page.get_by_text("Layer Mode").first.click()
            wait_idle(page)
        toms.click()
        page.keyboard.type("euclid_noise")
        page.get_by_role("option", name="euclid_noise_polymeter_4_4", exact=True).click()
        wait_idle(page)
        generate(page)
        assert_clean(page, "layer mode")
        cap = caption(page)
        results.append(("layer mode", cap))
        if "layered:" not in cap or "euclid_noise_polymeter_4_4" not in cap:
            raise AssertionError(f"layer mode did not engage: {cap!r}")
        shot(page, "4-layer.png")

        browser.close()

    for step, info in results:
        print(f"PASS  {step:12s} {info}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
