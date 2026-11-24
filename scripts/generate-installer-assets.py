#!/usr/bin/env python3
"""Generate brand derived assets for Windows packaging and the Chrome extension.

Produces:
  - assets/posthaze.ico (multi-resolution 16..256 ICO from App Icon Flat)
  - extensions/chrome/icons/icon-{16,32,48,128}.png (from App Icon Flat)
  - packaging/windows/assets/wizard-sidebar-light.png  (328x628, 2x 164x314)
  - packaging/windows/assets/wizard-sidebar-dark.png   (328x628, 2x 164x314)
  - packaging/windows/assets/wizard-small-light.png    (110x110, 2x 55x55)
  - packaging/windows/assets/wizard-small-dark.png     (110x110, 2x 55x55)

Color tokens follow DESIGN.md ("Chronophotographic Interval Plate").
"""

from __future__ import annotations

import struct
from io import BytesIO
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont

REPO_ROOT = Path(__file__).resolve().parent.parent
LOGO_PATH = REPO_ROOT / "assets" / "Logo Light" / "Logo Light 256x256.png"
FLAT_ICON_DIR = REPO_ROOT / "assets" / "App Icon Flat"
ICO_PATH = REPO_ROOT / "assets" / "posthaze.ico"
CHROME_ICONS_DIR = REPO_ROOT / "extensions" / "chrome" / "icons"
OUT_DIR = REPO_ROOT / "packaging" / "windows" / "assets"

AMBER = (0xE5, 0x8A, 0x35, 0xFF)       # selenium-amber #e58a35
CYAN = (0x3B, 0x82, 0xF6, 0xFF)        # cyanotype-blue #3b82f6
EMERALD = (0x22, 0xC5, 0x5E, 0xFF)     # halide-emerald #22c55e

DARK_THEME = {
    "bg": (0x14, 0x13, 0x11, 0xFF),         # graphite-bg #141311
    "header": (0x1B, 0x1A, 0x17, 0xFF),     # graphite-elevated #1b1a17
    "plate": (0x1E, 0x1D, 0x1A, 0xFF),      # table-bg #1e1d1a
    "border": (0x2A, 0x28, 0x25, 0xFF),     # graphite-border #2a2825
    "track": (0x0D, 0x0C, 0x0B, 0xFF),      # byte-track-bg #0d0c0b
    "text": (0xEA, 0xE6, 0xDF, 0xFF),       # silver-text #eae6df
    "muted": (0x9B, 0x96, 0x8D, 0xFF),      # silver-muted #9b968d
    "tick_minor": (0x34, 0x31, 0x2C, 0xFF),
    "tick_major": (0x54, 0x4F, 0x47, 0xFF),
    "logo_tint": (0xEA, 0xE6, 0xDF),
}

LIGHT_THEME = {
    "bg": (0xF7, 0xF5, 0xF0, 0xFF),         # warm editorial paper
    "header": (0xED, 0xEA, 0xE2, 0xFF),
    "plate": (0xFF, 0xFE, 0xFC, 0xFF),
    "border": (0xD8, 0xD3, 0xC8, 0xFF),
    "track": (0xE5, 0xE0, 0xD5, 0xFF),
    "text": (0x1C, 0x1B, 0x18, 0xFF),
    "muted": (0x6B, 0x66, 0x5D, 0xFF),
    "tick_minor": (0xCF, 0xC9, 0xBC, 0xFF),
    "tick_major": (0xA8, 0xA1, 0x93, 0xFF),
    "logo_tint": (0x1C, 0x1B, 0x18),
}


def load_font(candidates: list[str], size: int) -> ImageFont.FreeTypeFont | ImageFont.ImageFont:
    fonts_dir = Path("C:/Windows/Fonts")
    for name in candidates:
        path = fonts_dir / name
        if path.exists():
            return ImageFont.truetype(str(path), size=size)
    return ImageFont.load_default()


def tint_logo(logo: Image.Image, size: int, rgb: tuple[int, int, int]) -> Image.Image:
    resized = logo.resize((size, size), Image.Resampling.LANCZOS)
    alpha = resized.getchannel("A")
    tinted = Image.new("RGBA", (size, size), (*rgb, 255))
    tinted.putalpha(alpha)
    return tinted


def render_sidebar(theme: dict[str, tuple[int, int, int, int]], logo: Image.Image) -> Image.Image:
    w, h = 328, 628
    img = Image.new("RGBA", (w, h), theme["header"])
    draw = ImageDraw.Draw(img)

    # Right hairline divider separating the sidebar plate from the wizard content
    draw.line([(w - 2, 0), (w - 2, h)], fill=theme["border"], width=2)

    # Calibrated Chronophotographic vertical ruler along right edge
    ruler_x = w - 16
    for y in range(44, h - 44, 14):
        is_major = (y - 44) % 56 == 0
        length = 12 if is_major else 5
        color = theme["tick_major"] if is_major else theme["tick_minor"]
        draw.line([(ruler_x - length, y), (ruler_x, y)], fill=color, width=2 if is_major else 1)
    # Single Selenium Amber index mark + subtle Cyanotype reference mark
    draw.rectangle([ruler_x - 16, 156 - 2, ruler_x, 156 + 2], fill=AMBER)
    draw.rectangle([ruler_x - 12, 324 - 2, ruler_x, 324 + 2], fill=CYAN)

    # Top precision accent bar
    draw.rectangle([32, 36, 76, 41], fill=AMBER)

    # Precision emblem plate
    plate_size = 96
    plate_x, plate_y = 32, 68
    draw.rounded_rectangle(
        [plate_x, plate_y, plate_x + plate_size, plate_y + plate_size],
        radius=14,
        fill=theme["plate"],
        outline=theme["border"],
        width=2,
    )
    logo_size = 60
    resized_logo = tint_logo(logo, logo_size, theme["logo_tint"])
    logo_x = plate_x + (plate_size - logo_size) // 2
    logo_y = plate_y + (plate_size - logo_size) // 2
    img.alpha_composite(resized_logo, (logo_x, logo_y))

    # Wordmark
    font_title = load_font(["segoeuib.ttf", "arialbd.ttf"], 34)
    font_sub = load_font(["segoeui.ttf", "arial.ttf"], 16)
    font_mono = load_font(["consola.ttf", "segoeui.ttf"], 13)
    font_item = load_font(["segoeui.ttf", "arial.ttf"], 14)

    draw.text((32, 188), "Posthaze", font=font_title, fill=theme["text"])
    draw.text((32, 232), "Download Manager", font=font_sub, fill=theme["muted"])

    # Signature Graduated Byte-Map telemetry bar (DESIGN.md)
    bar_x, bar_y, bar_w, bar_h = 32, 286, 240, 14
    draw.rounded_rectangle(
        [bar_x, bar_y, bar_x + bar_w, bar_y + bar_h],
        radius=4,
        fill=theme["track"],
        outline=theme["border"],
        width=1,
    )
    segments = [
        (0.00, 0.25, AMBER),
        (0.27, 0.56, AMBER),
        (0.58, 0.79, CYAN),
        (0.81, 0.94, EMERALD),
    ]
    for s0, s1, col in segments:
        sx0 = bar_x + 2 + int((bar_w - 4) * s0)
        sx1 = bar_x + 2 + int((bar_w - 4) * s1)
        draw.rounded_rectangle([sx0, bar_y + 2, sx1, bar_y + bar_h - 2], radius=2, fill=col)

    draw.text((32, 312), "4 SEGMENTS  ·  SQLITE WAL", font=font_mono, fill=theme["muted"])

    # Clean bottom divider and grounded capability list
    draw.line([(32, 438), (w - 38, 438)], fill=theme["border"], width=2)
    items = [
        ("Parallel HTTP/HTTPS engine", 464),
        ("Crash-safe resume checkpoints", 494),
        ("Native CLI & Browser Capture", 524),
        ("Per-user install, no UAC", 554),
    ]
    for label, y in items:
        draw.rectangle([32, y + 7, 38, y + 13], fill=AMBER)
        draw.text((48, y), label, font=font_item, fill=theme["muted"])

    return img


def render_small_badge(theme: dict[str, tuple[int, int, int, int]], logo: Image.Image) -> Image.Image:
    size = 110
    # Match WizardBackColor / WizardBackColorDynamicDark so the top-right header badge
    # blends seamlessly with MainPanel under Windows 11 dynamic styling.
    img = Image.new("RGBA", (size, size), theme["bg"])
    draw = ImageDraw.Draw(img)

    margin = 10
    draw.rounded_rectangle(
        [margin, margin, size - margin, size - margin],
        radius=16,
        fill=theme["plate"],
        outline=theme["border"],
        width=2,
    )
    draw.rounded_rectangle([36, margin + 6, size - 36, margin + 10], radius=2, fill=AMBER)

    logo_size = 56
    resized_logo = tint_logo(logo, logo_size, theme["logo_tint"])
    offset = (size - logo_size) // 2
    img.alpha_composite(resized_logo, (offset, offset + 2))
    return img


def load_flat_icon(size: int) -> Image.Image:
    exact = FLAT_ICON_DIR / f"App Icon Flat {size}x{size}.png"
    if exact.exists():
        return Image.open(exact).convert("RGBA")
    source_size = 32 if size <= 32 else 64
    src = Image.open(FLAT_ICON_DIR / f"App Icon Flat {source_size}x{source_size}.png").convert("RGBA")
    return src.resize((size, size), Image.Resampling.LANCZOS)


def write_multi_size_ico(out_path: Path, sizes: list[int]) -> None:
    """Write a Windows ICO containing exact PNG frames for each requested size."""
    frames: list[tuple[int, bytes]] = []
    for size in sizes:
        buf = BytesIO()
        load_flat_icon(size).save(buf, format="PNG")
        frames.append((size, buf.getvalue()))

    header = struct.pack("<HHH", 0, 1, len(frames))
    offset = 6 + 16 * len(frames)
    entries = bytearray()
    payload = bytearray()
    for size, data in frames:
        dim = 0 if size >= 256 else size
        entries.extend(struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, len(data), offset))
        payload.extend(data)
        offset += len(data)

    out_path.write_bytes(header + entries + payload)


def generate_chrome_icons() -> None:
    CHROME_ICONS_DIR.mkdir(parents=True, exist_ok=True)
    for size in (16, 32, 48, 128):
        load_flat_icon(size).save(CHROME_ICONS_DIR / f"icon-{size}.png", format="PNG")


def main() -> None:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    logo = Image.open(LOGO_PATH).convert("RGBA")

    write_multi_size_ico(ICO_PATH, [16, 24, 32, 48, 64, 128, 256])
    generate_chrome_icons()

    render_sidebar(DARK_THEME, logo).save(OUT_DIR / "wizard-sidebar-dark.png", format="PNG")
    render_sidebar(LIGHT_THEME, logo).save(OUT_DIR / "wizard-sidebar-light.png", format="PNG")
    render_small_badge(DARK_THEME, logo).save(OUT_DIR / "wizard-small-dark.png", format="PNG")
    render_small_badge(LIGHT_THEME, logo).save(OUT_DIR / "wizard-small-light.png", format="PNG")
    print(f"Generated ICO ({ICO_PATH}), Chrome icons ({CHROME_ICONS_DIR}), and 4 installer PNGs ({OUT_DIR})")


if __name__ == "__main__":
    main()
