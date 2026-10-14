"""Draw the app icon: a realistic guitar pick ringed with "BRISTOL · 2026" around a raised dome.

    python tools/make_icon.py [--picks]      (Pillow and numpy; from ../rocksmith-player/tools/make_icon.py)

Writes the graphite pick (the app icon) to assets/icons/rsplay.png, .ico and .icns, and the amber celluloid pick
(kept for other uses) to assets/icons/rsplay-gold.png and .ico. --picks writes only the plain picks the sidebar
shows (gold in the dark themes, graphite in Daylight): assets/icons/pick-gold.png and pick-graphite.png. The ring
lettering only reads from about 64 px up, so the .ico holds the lettered pick for 64 px and larger and the plain
pick (grooves and dome, no text) for 16-48 px.

The geometry is the design mock-up's, in a 100-unit box; it is drawn at 4096 px and scaled down so edges stay smooth.
"""
import math
import sys
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFilter, ImageFont

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "assets" / "icons"
N = 1024          # final size
W = 4096          # drawing size
K = W / 100       # pixels per design unit
RING_TEXT = "BRISTOL · 2026 · BRISTOL · 2026 · "
FONTS = ["C:/Windows/Fonts/segoeuib.ttf", "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
         "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"]

# body gradient (light top-left to dark), bevel, gloss, ring grooves, lettering and dome, per finish
GRAPHITE = dict(hi=(74, 79, 87), mid=(44, 47, 53), lo=(22, 24, 27), bevel_hi=0.35, gloss=0.22,
                groove_dark=(16, 18, 20), groove_light=(74, 79, 87), ink=(185, 190, 199),
                dome_hi=(255, 217, 138), dome_lo=(185, 122, 20))
GOLD = dict(hi=(255, 217, 138), mid=(242, 181, 60), lo=(168, 103, 15), bevel_hi=0.55, gloss=0.5,
            groove_dark=(122, 74, 12), groove_light=(255, 231, 179), ink=(92, 55, 8),
            dome_hi=(90, 74, 53), dome_lo=(31, 26, 20))

# the pick and its bevel line (cubic beziers in design units, as in the mock-up)
PICK = [(50, 95.5), (43.5, 95.5, 7.5, 52, 7.5, 35.5), (7.5, 14, 27.5, 4.5, 50, 4.5),
        (72.5, 4.5, 92.5, 14, 92.5, 35.5), (92.5, 52, 56.5, 95.5, 50, 95.5)]
BEVEL = [(50, 92.5), (45, 92.5, 10.5, 51, 10.5, 35.5), (10.5, 16, 29, 7.5, 50, 7.5),
         (71, 7.5, 89.5, 16, 89.5, 35.5), (89.5, 51, 55, 92.5, 50, 92.5)]
GLOSS = [(18, 30), (20, 16, 34, 10, 50, 10), (60, 10, 68, 12, 73, 16), (60, 14, 40, 17, 30, 26),
         (26, 30, 22, 34, 18, 38)]
CX, CY = 50, 41   # ring and dome centre


def path_points(path, steps: int = 400) -> list[tuple[float, float]]:
    """A bezier path (start point, then (c1x, c1y, c2x, c2y, x, y) segments) as pixel points."""
    x0, y0 = path[0]
    pts = []
    for seg in path[1:]:
        c1x, c1y, c2x, c2y, x, y = seg
        for i in range(steps):
            t = i / steps
            a, b, c, d = (1 - t) ** 3, 3 * (1 - t) ** 2 * t, 3 * (1 - t) * t * t, t ** 3
            pts.append(((a * x0 + b * c1x + c * c2x + d * x) * K, (a * y0 + b * c1y + c * c2y + d * y) * K))
        x0, y0 = x, y
    return pts


def mask_of(points) -> Image.Image:
    m = Image.new("L", (W, W), 0)
    ImageDraw.Draw(m).polygon(points, fill=255)
    return m


def solid(rgb, alpha: Image.Image) -> Image.Image:
    layer = Image.new("RGBA", alpha.size, rgb + (0,))
    layer.putalpha(alpha)
    return layer


def gradient(stops, field: np.ndarray) -> np.ndarray:
    """field (0..1 per pixel) through colour stops [(pos, (r, g, b)), ...] -> HxWx3 float."""
    out = np.zeros(field.shape + (3,))
    pos = [p for p, _ in stops]
    for ch in range(3):
        out[..., ch] = np.interp(field, pos, [c[ch] for _, c in stops])
    return out


def radial(cx, cy, r) -> np.ndarray:
    """Distance from (cx, cy) in units of r, per pixel (all in design units)."""
    ys, xs = np.mgrid[0:W, 0:W].astype(np.float32) / K
    return np.clip(np.hypot(xs - cx, ys - cy) / r, 0, 1)


def font(size: float):
    for f in FONTS:
        if Path(f).is_file():
            return ImageFont.truetype(f, round(size))
    return ImageFont.load_default(round(size))


def ring_text(img: Image.Image, colour, r: float, size: float, dy: float = 0.0) -> None:
    """RING_TEXT round a circle of radius r (design units), starting at 9 o'clock and running clockwise over the
    top, upright and reading outwards (like SVG text on a clockwise path), spaced to fill the circle."""
    f = font(size * K)
    widths = [f.getlength(ch) for ch in RING_TEXT]
    circumference = 2 * math.pi * r * K
    gap = (circumference - sum(widths)) / len(RING_TEXT)
    s = 0.0
    for ch, w in zip(RING_TEXT, widths):
        phi = (s + w / 2) / (r * K)  # arc angle from 9 o'clock
        s += w + gap
        if not ch.strip():
            continue
        x = CX * K - r * K * math.cos(phi)
        y = (CY + dy) * K - r * K * math.sin(phi)
        g = Image.new("L", (int(w) + 40, int(size * K * 2)), 0)
        ImageDraw.Draw(g).text((g.width / 2, g.height / 2), ch, font=f, fill=255, anchor="ms")  # baseline at centre
        g = g.rotate(90 - math.degrees(phi), resample=Image.BICUBIC, expand=True)
        img.alpha_composite(solid(colour, g), (round(x - g.width / 2), round(y - g.height / 2)))


def draw(fin: dict, lettered: bool) -> Image.Image:
    img = Image.new("RGBA", (W, W), (0, 0, 0, 0))
    pick = mask_of(path_points(PICK))

    # soft shadow under the pick, so it sits above whatever is behind it
    shadow = pick.filter(ImageFilter.GaussianBlur(2.6 * K)).point(lambda v: round(v * 0.45))
    img.alpha_composite(solid((0, 0, 0), shadow), (round(1.6 * K), round(3 * K)))

    # the body: lit from the top left
    body = gradient([(0, fin["hi"]), (0.55, fin["mid"]), (1, fin["lo"])], radial(7.5 + 85 * 0.32, 4.5 + 91 * 0.22, 0.95 * 91))
    layer = Image.fromarray(body.astype(np.uint8), "RGB").convert("RGBA")
    layer.putalpha(pick)
    img.alpha_composite(layer)

    # bevel: a line just inside the edge, bright at the top left and dark at the bottom right
    line = Image.new("L", (W, W), 0)
    pts = path_points(BEVEL)
    ImageDraw.Draw(line).line(pts + [pts[0]], fill=255, width=round(2.2 * K), joint="curve")
    ys, xs = np.mgrid[0:W, 0:W].astype(np.float32) / K
    t = np.clip(((xs - 10.5) + (ys - 7.5)) / (79 + 85), 0, 1)
    hi = np.clip(1 - t / 0.5, 0, 1) * fin["bevel_hi"]
    lo = np.clip((t - 0.5) / 0.5, 0, 1) * 0.45
    la = np.asarray(line, np.float32) / 255
    img.alpha_composite(solid((255, 255, 255), Image.fromarray((la * hi * 255).astype(np.uint8))))
    img.alpha_composite(solid((0, 0, 0), Image.fromarray((la * lo * 255).astype(np.uint8))))

    # gloss across the upper left
    gloss = np.asarray(mask_of(path_points(GLOSS)), np.float32) / 255
    gt = np.clip(((xs - 18) * 0.6 + (ys - 10)) / (55 * 0.6 + 28), 0, 1)
    img.alpha_composite(solid((255, 255, 255), Image.fromarray((gloss * (1 - gt) * fin["gloss"] * 255).astype(np.uint8))))

    # the seal: two grooves pressed into the pick (dark edge, light edge just below)
    d = ImageDraw.Draw(img)
    for r, wd, wl in ((28.5, 1.4, 0.8), (16.5, 1.2, 0.7)):
        for dy, col, width in ((0.7, fin["groove_light"], wl), (0, fin["groove_dark"], wd)):
            d.ellipse(((CX - r) * K, (CY + dy - r) * K, (CX + r) * K, (CY + dy + r) * K), outline=col + (255,),
                      width=round(width * K))
    if lettered:
        ring_text(img, fin["groove_light"], 22.5, 6.3, dy=0.55)
        ring_text(img, fin["ink"], 22.5, 6.3)

    # the dome
    dome = mask_of([(CX * K + 12 * K * math.cos(a / 200 * math.tau), CY * K + 12 * K * math.sin(a / 200 * math.tau))
                    for a in range(200)])
    ds = dome.filter(ImageFilter.GaussianBlur(2.6 * K)).point(lambda v: round(v * 0.45))
    img.alpha_composite(solid((0, 0, 0), ds), (round(1.6 * K), round(3 * K)))
    dg = gradient([(0, fin["dome_hi"]), (1, fin["dome_lo"])], radial(CX - 12 + 24 * 0.35, CY - 12 + 24 * 0.3, 0.8 * 24))
    dl = Image.fromarray(dg.astype(np.uint8), "RGB").convert("RGBA")
    dl.putalpha(dome)
    img.alpha_composite(dl)
    shine = Image.new("L", (W, W), 0)
    ImageDraw.Draw(shine).ellipse(((46.5 - 5) * K, (37 - 3) * K, (46.5 + 5) * K, (37 + 3) * K), fill=round(255 * 0.35))
    img.alpha_composite(solid((255, 255, 255), shine))

    return img.resize((N, N), Image.LANCZOS)


ICO_SMALL = [(16, 16), (24, 24), (32, 32), (48, 48)]
ICO_LARGE = [(64, 64), (128, 128), (256, 256)]


def save_ico(path: Path, lettered: Image.Image, plain: Image.Image) -> None:
    """Each size its own image: the plain pick where the lettering can't be read, the lettered one above."""
    images = [plain.resize(s, Image.LANCZOS) for s in ICO_SMALL] + [lettered.resize(s, Image.LANCZOS) for s in ICO_LARGE]
    images[-1].save(path, sizes=ICO_SMALL + ICO_LARGE, append_images=images[:-1])


def main() -> None:
    if "--picks" in sys.argv[1:]:
        for fin, name in ((GOLD, "pick-gold"), (GRAPHITE, "pick-graphite")):
            draw(fin, False).resize((256, 256), Image.LANCZOS).save(OUT / f"{name}.png")
            print(f"wrote {OUT / name}.png")
        return
    for fin, name in ((GRAPHITE, "rsplay"), (GOLD, "rsplay-gold")):
        lettered, plain = draw(fin, True), draw(fin, False)
        lettered.save(OUT / f"{name}.png")
        save_ico(OUT / f"{name}.ico", lettered, plain)
        if name == "rsplay":
            lettered.save(OUT / f"{name}.icns")
        print(f"wrote {OUT / name}.png and .ico")


if __name__ == "__main__":
    main()
