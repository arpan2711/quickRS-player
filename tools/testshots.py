"""Screenshots of the app for checking a change, fast: a small test library, one launch for many shots, an optimised
build that rebuilds quickly, and before/after comparisons.

    python tools/testshots.py --shots 12f_tuner_arc,12f_tuner_needle --song "Love in an Elevator"
    python tools/testshots.py --shots 13l_highway --song Elevator --seq 60,60.5 --before --compare
    python tools/testshots.py --shots 13l_highway_full --set hw_full_score=false --judge hit --seq 60,60.5

What it does:
  - builds quickrs with the `fast` profile (target/fast; never touches target/release, the owner's shortcut), unless
    --no-build;
  - runs it in screenshot mode on the test library's data (QUICKRS_DATA = ../quickRS-testlib/data: its config's
    library is ../quickRS-testlib/songs only, RS's own dlc folder is not searched, so start-up doesn't scan 3,000
    songs); --set key=value changes that config for this run only (put back after);
  - --before also renders the owner's build (target/release/quickrs.exe) the same way, --compare puts each pair side
    by side (--crop x0,y0,x1,y1 first) in OUT/compare/.

The test library (outside the repo: real songs never go in git) is made on first use from these songs; add one with
--add PATH (a .psarc or .feedpak). Shot names are in crates/qrs-app/src/shots.rs; QUICKRS_SHOT_SEQ times (--seq,
rising, comma-separated) render the shot at each song time instead of once.
"""
import argparse
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
LIB = ROOT.parent / "quickRS-testlib"
SONGS = LIB / "songs"
DATA = LIB / "data"
KEYS = ROOT.parent / "rocksmith-player" / "keys.json"
FAST = ROOT / "target" / "fast" / "quickrs.exe"
OWNER = ROOT / "target" / "release" / "quickrs.exe"


def ensure_lib():
    SONGS.mkdir(parents=True, exist_ok=True)
    DATA.mkdir(parents=True, exist_ok=True)
    cfg = DATA / "config.json"
    c = json.loads(cfg.read_text(encoding="utf-8")) if cfg.exists() else {}
    c["library_dirs"] = [str(SONGS)]
    c["search_rocksmith_dlc"] = False
    cfg.write_text(json.dumps(c, indent=2), encoding="utf-8")
    if not (DATA / "keys.json").exists() and KEYS.exists():
        shutil.copy(KEYS, DATA / "keys.json")


def parse_value(v):
    try:
        return json.loads(v)
    except ValueError:
        return v


def render(exe, out, a):
    out.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ)
    env["QUICKRS_DATA"] = str(DATA)
    env["QUICKRS_SHOTS"] = a.shots
    for k in ("QUICKRS_SHOT_SONG", "QUICKRS_SHOT_SEQ", "QUICKRS_SHOT_JUDGE", "QUICKRS_SHOT_PART", "QUICKRS_SHOT_AT"):
        env.pop(k, None)
    if a.part:
        env["QUICKRS_SHOT_PART"] = a.part
    if a.at is not None:
        env["QUICKRS_SHOT_AT"] = str(a.at)
    if a.song:
        env["QUICKRS_SHOT_SONG"] = a.song
    if a.seq:
        env["QUICKRS_SHOT_SEQ"] = a.seq if ("," in a.seq or ":" in a.seq) else a.seq + ","
    if a.judge:
        env["QUICKRS_SHOT_JUDGE"] = a.judge
    r = subprocess.run([str(exe), "--screenshots", str(out), "--size", a.size], env=env,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=600)
    files = sorted(out.glob("*.png"))
    print(f"{exe.parent.name}: {len(files)} image(s) in {out}" + (f" (exit {r.returncode})" if r.returncode else ""))
    return files


def compare(before, after, out, crop):
    try:
        from PIL import Image, ImageDraw, ImageFont
    except ImportError:
        sys.exit("--compare needs Pillow (pip install pillow)")
    out.mkdir(parents=True, exist_ok=True)
    try:
        font = ImageFont.truetype("C:/Windows/Fonts/arialbd.ttf", 22)
    except OSError:
        font = ImageFont.load_default()
    for b in before:
        f = after.parent / b.name if after else None
        if not f or not f.exists():
            continue
        ims = [Image.open(b), Image.open(f)]
        if crop:
            ims = [im.crop(crop) for im in ims]
        w, h = ims[0].size
        img = Image.new("RGB", (w * 2 + 10, h + 36), (0, 0, 0))
        d = ImageDraw.Draw(img)
        for k, (im, lab) in enumerate(zip(ims, ("BEFORE", "AFTER"))):
            img.paste(im, (k * (w + 10), 36))
            d.text((k * (w + 10) + 8, 6), lab, font=font, fill=(255, 255, 255))
        img.save(out / b.name)
    print(f"comparisons in {out}")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--shots", help="shot names, comma-separated (an exact name takes only that shot)")
    ap.add_argument("--song", help="part of the song's title")
    ap.add_argument("--seq", help="song times (rising, comma-separated) or t0:t1:fps")
    ap.add_argument("--part", help="the part's key: lead, rhythm, bass, combo, lead2, ...")
    ap.add_argument("--at", type=float, help="the moment of the song, in seconds (QUICKRS_SHOT_AT)")
    ap.add_argument("--judge", choices=["hit", "late", "early", "miss"])
    ap.add_argument("--set", action="append", default=[], metavar="KEY=VALUE", help="config for this run only")
    ap.add_argument("--size", default="1600x900")
    ap.add_argument("--out", help="where the images go (default: ../quickRS-testlib/shots/last)")
    ap.add_argument("--before", action="store_true", help="also render the owner's build (target/release)")
    ap.add_argument("--compare", action="store_true", help="before/after side by side (with --before)")
    ap.add_argument("--crop", help="x0,y0,x1,y1 for the comparisons")
    ap.add_argument("--no-build", action="store_true")
    ap.add_argument("--add", action="append", default=[], metavar="SONG", help="copy a song into the test library")
    a = ap.parse_args()

    ensure_lib()
    for s in a.add:
        shutil.copy(s, SONGS / Path(s).name)
        print(f"added {Path(s).name}")
    if not a.shots:
        if not a.add:
            ap.error("--shots is needed")
        return
    if not a.no_build:
        r = subprocess.run(["cargo", "build", "--profile", "fast", "-p", "qrs-app", "--bin", "quickrs"], cwd=ROOT)
        if r.returncode:
            sys.exit("build failed")
    out = Path(a.out) if a.out else LIB / "shots" / "last"
    if ROOT in out.resolve().parents:
        sys.exit("--out is inside the repo: real songs' pictures never go in git")
    shutil.rmtree(out, ignore_errors=True)

    cfg = DATA / "config.json"
    saved = cfg.read_text(encoding="utf-8")
    try:
        if a.set:
            c = json.loads(saved)
            for kv in a.set:
                k, _, v = kv.partition("=")
                c[k] = parse_value(v)
            cfg.write_text(json.dumps(c, indent=2), encoding="utf-8")
        before = render(OWNER, out / "before", a) if a.before else []
        after = render(FAST, out / "after", a)
    finally:
        # (the run's settings, and anything the app saved, go back to how the test library had them)
        cfg.write_text(saved, encoding="utf-8")
    if a.compare and before and after:
        crop = tuple(int(v) for v in a.crop.split(",")) if a.crop else None
        compare(before, after[0], out / "compare", crop)


if __name__ == "__main__":
    main()
