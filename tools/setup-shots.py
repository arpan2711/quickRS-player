"""Make the first-time setup's pictures (assets/setup/*.png, P23.14) from the app's own screenshots.

    python tools/setup-shots.py [--exe target/share/release/quickrs.exe] [--scratch C:/quickRS-shots] [--keep]

Use the shared build (`cargo build --release -p qrs-app --no-default-features --target-dir target/share`): no
brand names, no MG-300. It makes a scratch data folder (never your own: QUICKRS_DATA points at it) whose library is
the synthetic test feedpaks, with neutral folders for songs and recordings and RS's dlc folders left out, runs
`quickrs --screenshots` there with the stand-in guitar named "Rocksmith Guitar Adapter", crops what each page is
about, and saves the crops small (an adaptive palette). Look at every picture after making them.
"""

import argparse
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
FIXTURES = ROOT / "crates/qrs-formats/tests/fixtures/feedpak"
OUT = ROOT / "assets/setup"

# (picture, shot, theme, crop box at 1440 x 900, width it's saved at)
CROPS = [
    # the calibration's sliders (Settings > Audio's guitar setup, step 6)
    ("guitar", "95e3_setup_settings", "graphite", (386, 496, 1310, 756), 620),
    # Settings > Timing > Reduce delay (P35.4; lower since P36: the Calibrate timing and Screen cards above it): Low-latency output (its FAST badge), the buffer, the latency chip
    ("latency", "20d_settings_low_latency", "graphite", (386, 580, 1310, 760), 620),
    # the note widget on its own
    ("widget", "41_note_widget", "graphite", (667, 120, 1400, 320), 620),
    # Settings > Directories' song folders (the keys' "looked in" line, with this PC's paths, left out)
    ("folders", "91_settings_library_folders", "graphite", (386, 130, 1166, 400), 600),
    # the amp room as one screen (P37.1, P38.1): the chain and the open unit
    ("room", "94_room", "graphite", (20, 85, 1420, 590), 620),
    # each theme: the sidebar and Home's first card
    ("theme-midnight", "00_home", "midnight", (28, 25, 700, 470), 400),
    ("theme-graphite", "00_home", "graphite", (28, 25, 700, 470), 400),
    ("theme-daylight", "00_home", "daylight", (28, 25, 700, 470), 400),
]
BOXES = {c[0]: c[3] for c in CROPS}


def scratch_data(scratch: Path) -> Path:
    data = scratch / "quickRS Data"
    music = scratch / "Music"
    songs = music / "quickRS Songs"
    for d in (data, songs, music / "quickRS Recordings", music / "quickRS Takes"):
        d.mkdir(parents=True, exist_ok=True)
    for pack in FIXTURES.glob("*.feedpak"):
        dst = songs / pack.name
        if not dst.exists():
            shutil.copytree(pack, dst)
    removed = []
    for dlc in (r"Y:\Rocksmith 2014 Edition - Remastered\dlc",
                r"C:\Program Files (x86)\Steam\steamapps\common\Rocksmith2014\dlc"):
        removed += [dlc, str(Path(dlc).parent / "feedpak")]
    cfg = {
        "library_dirs": [str(songs)],
        "library_dirs_removed": removed,
        "search_rocksmith_dlc": False,
        "recordings_dir": str(music / "quickRS Recordings"),
        "takes_dir": str(music / "quickRS Takes"),
        "first_setup": "done",
    }
    (data / "config.json").write_text(json.dumps(cfg, indent=2), encoding="utf-8")
    return data


def shoot(exe: Path, data: Path, out: Path, theme: str, names):
    env = dict(os.environ)
    env.update({
        "QUICKRS_DATA": str(data),
        "QUICKRS_SHOT_DEVICE": "Rocksmith Guitar Adapter",
        "QUICKRS_SHOTS": ",".join(names),
    })
    out.mkdir(parents=True, exist_ok=True)
    # (nothing played yet: Home's first card is its welcome, not a test song's red cover)
    for f in ("userdata.json", "userdata.json.bak"):
        (data / f).unlink(missing_ok=True)
    subprocess.run([str(exe), "--screenshots", str(out), "--size", "1440x900", "--theme", theme],
                   env=env, check=True, timeout=900)


def save_small(img: Image.Image, width: int, path: Path):
    if img.width > width:
        img = img.resize((width, round(img.height * width / img.width)), Image.LANCZOS)
    img.convert("RGB").quantize(colors=128, method=Image.MEDIANCUT, dither=Image.NONE).save(path, optimize=True)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", default=str(ROOT / "target/share/release/quickrs.exe"))
    ap.add_argument("--scratch", default=r"C:\quickRS-shots")
    ap.add_argument("--keep", action="store_true", help="keep the scratch folder")
    ap.add_argument("--only-shots", action="store_true", help="take the screenshots, crop nothing")
    a = ap.parse_args()
    scratch = Path(a.scratch)
    data = scratch_data(scratch)
    shots = scratch / "shots"
    for theme in sorted({c[2] for c in CROPS}):
        names = sorted({c[1] for c in CROPS if c[2] == theme})
        shoot(Path(a.exe), data, shots / theme, theme, names)
    if a.only_shots:
        print(f"shots in {shots}")
        return
    OUT.mkdir(parents=True, exist_ok=True)
    total = 0
    for pic, shot, theme, _, width in CROPS:
        src = shots / theme / f"{shot}.png"
        img = Image.open(src)
        box = BOXES.get(pic)
        if box:
            img = img.crop(box)
        dst = OUT / f"{pic}.png"
        save_small(img, width, dst)
        total += dst.stat().st_size
        print(f"{dst.name}: {Image.open(dst).size} {dst.stat().st_size // 1024} KB")
    print(f"{total // 1024} KB in all")
    if not a.keep:
        shutil.rmtree(scratch, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main())
