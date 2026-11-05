"""Render a stretch of a song as a frame sequence and put it beside (or over) a reference video, for comparing
motion frame by frame (the highway against RS's own, say).

    python tools/hwclip.py --data SCRATCH_DATA --song "Take Me Out" --from 58 --to 66 \\
        --ref reference.mp4 --out SCRATCH_DIR [--fps 30] [--page 13l_highway] [--judge hit] [--stack]

It runs quickrs.exe in screenshot mode with QUICKRS_SHOT_SEQ (frames held at exact song times, see
crates/qrs-app/src/shots.rs), then stitches the frames and the reference's same stretch with ffmpeg into
OUT/compare.mp4. Find the reference's offset first (--ref-offset: video time minus song time; a screen recording
rarely starts at the song's 0, e.g. by cross-correlating its audio with the song's). Real songs and their videos never go in the repo:
point --out and --data at a scratch folder.
"""
import argparse
import os
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def ffmpeg_exe(given):
    if given:
        return given
    try:
        import imageio_ffmpeg  # bundled ffmpeg (pip install imageio-ffmpeg)

        return imageio_ffmpeg.get_ffmpeg_exe()
    except ImportError:
        found = shutil.which("ffmpeg")
        if not found:
            sys.exit("no ffmpeg: pip install imageio-ffmpeg, or pass --ffmpeg")
        return found


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--data", required=True, help="QUICKRS_DATA: a scratch data folder whose library has the song")
    ap.add_argument("--song", required=True, help="part of the song's title (QUICKRS_SHOT_SONG)")
    ap.add_argument("--from", dest="t0", type=float, required=True, help="start, song seconds")
    ap.add_argument("--to", dest="t1", type=float, required=True, help="end, song seconds")
    ap.add_argument("--fps", type=float, default=30.0)
    ap.add_argument("--page", default="13l_highway", help="the shot to render (an exact shot name)")
    ap.add_argument("--judge", choices=["hit", "late", "early", "miss", "none"], default="hit",
                    help="how each note is judged as it passes (none: unplayed, so missed)")
    ap.add_argument("--ref", help="the reference video (its time = song time unless --ref-offset)")
    ap.add_argument("--ref-offset", type=float, default=0.0,
                    help="reference video time minus song time, seconds. CHECK IT: a recording rarely starts at the song's "
                         "0 (the owner's Take Me Out video is -9.57: song time = video time + 9.57)")
    ap.add_argument("--out", required=True, help="output folder (scratch, never the repo)")
    ap.add_argument("--size", default="1280x720")
    ap.add_argument("--stack", action="store_true", help="one above the other instead of side by side")
    ap.add_argument("--crop", help="crop quickRS's frames first, W:H:X:Y in the render's pixels (e.g. the highway's well)")
    ap.add_argument("--exe", default=str(ROOT / "target" / "release" / "quickrs.exe"))
    ap.add_argument("--ffmpeg", help="ffmpeg to use (default: imageio-ffmpeg's, else ffmpeg on PATH)")
    ap.add_argument("--keep-frames", action="store_true", help="keep an earlier render's frames (no re-render)")
    a = ap.parse_args()

    out = Path(a.out).resolve()
    if ROOT in out.parents or out == ROOT:
        sys.exit("--out is inside the repo: use a scratch folder (no song material in git)")
    frames = out / "frames"
    if not a.keep_frames:
        shutil.rmtree(frames, ignore_errors=True)
        frames.mkdir(parents=True, exist_ok=True)
        env = dict(os.environ)
        env.update({
            "QUICKRS_DATA": a.data,
            "QUICKRS_SHOT_SONG": a.song,
            "QUICKRS_SHOTS": a.page,
            "QUICKRS_SHOT_SEQ": f"{a.t0}:{a.t1}:{a.fps}",
        })
        if a.judge != "none":
            env["QUICKRS_SHOT_JUDGE"] = a.judge
        else:
            env.pop("QUICKRS_SHOT_JUDGE", None)
        print(f"rendering {a.page}, {a.t0}-{a.t1} s at {a.fps} fps ...", flush=True)
        exe = str(Path(a.exe).resolve())
        if not Path(exe).exists():
            sys.exit(f"no {exe}: build it (cargo build --release -p qrs-app) or pass --exe")
        r = subprocess.run([exe, "--screenshots", str(frames), "--size", a.size], env=env,
                           stdout=subprocess.DEVNULL)
        if r.returncode != 0:
            sys.exit(f"quickrs exited with {r.returncode}")
    n = len(list(frames.glob("frame_*.png")))
    if n == 0:
        sys.exit(f"no frames in {frames} (is --page an exact shot name, and --song in the data's library?)")
    print(f"{n} frames", flush=True)
    if not a.ref:
        return
    ff = ffmpeg_exe(a.ffmpeg)
    w, h = a.size.split("x")
    dur = a.t1 - a.t0 + 1.0 / a.fps
    stack = "vstack" if a.stack else "hstack"
    crop = f"crop={a.crop}," if a.crop else ""
    font = "C\:/Windows/Fonts/arialbd.ttf"
    label = lambda t: f"drawtext=fontfile='{font}':text='{t}':x=12:y=12:fontcolor=white:fontsize=24:box=1:boxcolor=black@0.5"
    cmd = [
        ff, "-y", "-loglevel", "error",
        "-framerate", str(a.fps), "-i", str(frames / "frame_%05d.png"),
        "-ss", str(a.t0 + a.ref_offset), "-t", str(dur), "-i", a.ref,
        "-filter_complex",
        f"[0:v]{crop}scale={w}:{h},setpts=PTS-STARTPTS,{label('quickRS')}[a];"
        f"[1:v]scale={w}:{h},fps={a.fps},setpts=PTS-STARTPTS,{label('reference')}[b];"
        f"[a][b]{stack}=inputs=2[v]",
        "-map", "[v]", "-an", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-crf", "18",
        str(out / "compare.mp4"),
    ]
    r = subprocess.run(cmd)
    if r.returncode != 0:
        # (an ffmpeg without drawtext's fonts: the same without labels)
        cmd[cmd.index("-filter_complex") + 1] = (
            f"[0:v]{crop}scale={w}:{h},setpts=PTS-STARTPTS[a];[1:v]scale={w}:{h},fps={a.fps},setpts=PTS-STARTPTS[b];"
            f"[a][b]{stack}=inputs=2[v]"
        )
        r = subprocess.run(cmd)
        if r.returncode != 0:
            sys.exit("ffmpeg failed")
    print(out / "compare.mp4")


if __name__ == "__main__":
    main()
