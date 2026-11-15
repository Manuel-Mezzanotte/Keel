#!/usr/bin/env python3
"""Generates Keel's mark and app icon from the paper boat glyph.

The geometry in this file is the single source of truth for the logo. The
outputs are committed next to this file, so a normal build never runs this
script. Run it again only after changing the logo:

    python3 assets/brand/generate.py

Requires ImageMagick (`magick`) and `iconutil` (part of macOS).

The logo is a paper boat folded from a page of code: the boat stands for
the name, the coloured lines are Keel's syntax colours. The tile uses the
midnight blues of Sail, Manuel's browser, so the two apps look like one
family: Sail is the sail, Keel is the boat.
"""

import shutil
import subprocess
import tempfile
from pathlib import Path

OUT = Path(__file__).resolve().parent

# Apple's icon grid: a 1024 canvas with an 824 tile whose corners are 185.4.
APPLE_MARGIN = 100
TILE = 1024 - 2 * APPLE_MARGIN
RADIUS = 185.4
# Sail's "blu-notte" palette: gradient top and bottom.
TILE_TOP = '#1E2B47'
TILE_BOTTOM = '#070B14'

# The boat on the 1024 canvas: a sail and a hull, each folded in two, with
# the light coming from the left.
SAIL_TOP = (512, 220)
SAIL_BASE = 548
SAIL_HALF = 160
HULL_TOP = 548
HULL_BOTTOM = 760
HULL_HALF_TOP = 362
HULL_HALF_BOTTOM = 226

FACETS = {
    'sail-left': (f'M512 {SAIL_TOP[1]} L {512 - SAIL_HALF} {SAIL_BASE} H 512 Z', '#FFFFFF'),
    'sail-right': (f'M512 {SAIL_TOP[1]} L 512 {SAIL_BASE} H {512 + SAIL_HALF} Z', '#B3C8EC'),
    'hull-left': (f'M{512 - HULL_HALF_TOP} {HULL_TOP} H 512 V {HULL_BOTTOM} '
                  f'H {512 - HULL_HALF_BOTTOM} Z', '#E4EDFB'),
    'hull-right': (f'M512 {HULL_TOP} H {512 + HULL_HALF_TOP} L {512 + HULL_HALF_BOTTOM} '
                   f'{HULL_BOTTOM} H 512 Z', '#86A3D8'),
}

PINK, BLUE, GREEN, VIOLET = '#FF5FA2', '#3B82F6', '#1F9D7A', '#8B5CF6'
SHADE = '#24365E'
# The lines of code printed on the paper: (facet, x, y, width, colour,
# opacity). Each facet clips its lines, as a fold would.
CODE = [
    ('sail-left', 430, 380, 90, PINK, 0.85),
    ('sail-left', 400, 420, 60, BLUE, 0.85),
    ('sail-left', 470, 420, 50, GREEN, 0.85),
    ('sail-left', 380, 460, 140, VIOLET, 0.7),
    ('sail-left', 360, 500, 70, PINK, 0.85),
    ('sail-left', 440, 500, 80, BLUE, 0.8),
    ('sail-right', 520, 380, 40, SHADE, 0.32),
    ('sail-right', 520, 420, 90, SHADE, 0.32),
    ('sail-right', 520, 460, 70, SHADE, 0.32),
    ('sail-right', 520, 500, 110, SHADE, 0.32),
    ('hull-left', 200, 590, 120, BLUE, 0.75),
    ('hull-left', 330, 590, 90, PINK, 0.8),
    ('hull-left', 260, 630, 160, GREEN, 0.75),
    ('hull-left', 260, 670, 70, VIOLET, 0.7),
    ('hull-left', 340, 670, 130, BLUE, 0.7),
    ('hull-left', 300, 710, 100, PINK, 0.75),
    ('hull-right', 540, 590, 150, SHADE, 0.28),
    ('hull-right', 540, 630, 220, SHADE, 0.28),
    ('hull-right', 540, 670, 120, SHADE, 0.28),
    ('hull-right', 540, 710, 160, SHADE, 0.28),
]
LINE_HEIGHT = 20

# Two strokes of water under the boat.
WATER = [(286, 738, 800, 0.5), (360, 664, 846, 0.25)]
WATER_COLOUR = '#6EA8FE'
WATER_WIDTH = 18


def svg(width, height, body, view_box='0 0 1024 1024'):
    return (f'<svg width="{width}" height="{height}" viewBox="{view_box}" '
            f'fill="none" xmlns="http://www.w3.org/2000/svg">{body}</svg>\n')


def boat():
    """The boat with its code and the water, in flat colours."""
    clips = ''.join(f'<clipPath id="{name}"><path d="{path}"/></clipPath>'
                    for name, (path, _) in FACETS.items())
    facets = ''.join(f'<path d="{path}" fill="{colour}"/>' for path, colour in FACETS.values())
    code = ''
    for facet in FACETS:
        lines = ''.join(
            f'<rect x="{x}" y="{y}" width="{width}" height="{LINE_HEIGHT}" '
            f'rx="{LINE_HEIGHT // 2}" fill="{colour}" fill-opacity="{opacity}"/>'
            for name, x, y, width, colour, opacity in CODE if name == facet)
        code += f'<g clip-path="url(#{facet})">{lines}</g>'
    # Rounded bars rather than strokes with round caps: ImageMagick draws
    # the caps over the line, which darkens the ends of see-through strokes.
    half = WATER_WIDTH / 2
    water = ''.join(
        f'<rect x="{left - half}" y="{y - half}" width="{right - left + WATER_WIDTH}" '
        f'height="{WATER_WIDTH}" rx="{half}" fill="{WATER_COLOUR}" fill-opacity="{opacity}"/>'
        for left, right, y, opacity in WATER)
    return f'<defs>{clips}</defs>{facets}{code}{water}'


def tile_edge():
    """A faint light line along the tile's edge."""
    inset = 1.5
    return (f'<rect x="{APPLE_MARGIN + inset}" y="{APPLE_MARGIN + inset}" '
            f'width="{TILE - 2 * inset}" height="{TILE - 2 * inset}" rx="{RADIUS - inset}" '
            f'stroke="#FFFFFF" stroke-opacity="0.08" stroke-width="3"/>')


def mark_svg():
    """The boat's outline in one colour, for use inside Keel: sail and hull
    with a gap between them, so it reads at 16 px."""
    gap = 28
    sail = f'M512 {SAIL_TOP[1]} L {512 - SAIL_HALF} {SAIL_BASE} H {512 + SAIL_HALF} Z'
    hull = (f'M{512 - HULL_HALF_TOP} {HULL_TOP + gap} H {512 + HULL_HALF_TOP} '
            f'L {512 + HULL_HALF_BOTTOM} {HULL_BOTTOM} H {512 - HULL_HALF_BOTTOM} Z')
    body = f'<path d="{sail}" fill="currentColor"/><path d="{hull}" fill="currentColor"/>'
    return svg(24, 24, body, view_box='130 108 764 764')


def icon_svg():
    """The vector master: tile and boat on Apple's 1024 grid."""
    defs = ('<defs><linearGradient id="tile" x1="0" y1="0" x2="0" y2="1">'
            f'<stop offset="0" stop-color="{TILE_TOP}"/>'
            f'<stop offset="1" stop-color="{TILE_BOTTOM}"/></linearGradient>'
            '<linearGradient id="sheen" x1="0" y1="0" x2="0" y2="1">'
            '<stop offset="0" stop-color="#FFFFFF" stop-opacity="0.10"/>'
            '<stop offset="0.5" stop-color="#FFFFFF" stop-opacity="0"/></linearGradient></defs>')
    tile = (f'<rect x="{APPLE_MARGIN}" y="{APPLE_MARGIN}" width="{TILE}" height="{TILE}" '
            f'rx="{RADIUS}" fill="url(#tile)"/>'
            f'<rect x="{APPLE_MARGIN}" y="{APPLE_MARGIN}" width="{TILE}" height="{TILE}" '
            f'rx="{RADIUS}" fill="url(#sheen)"/>')
    return svg(1024, 1024, defs + tile + tile_edge() + boat())


def magick(*args):
    subprocess.run(['magick', *map(str, args)], check=True)


def icon_png(out, supersample=4):
    """The 1024 icon. ImageMagick's SVG renderer ignores gradients, so the
    tile and its sheen are drawn with ImageMagick and the boat on top."""
    canvas = 1024
    big = canvas * supersample
    tile_px = TILE * supersample
    radius = RADIUS * supersample
    offset = APPLE_MARGIN * supersample
    with tempfile.TemporaryDirectory() as tmp:
        glyph_svg = Path(tmp) / 'glyph.svg'
        glyph_svg.write_text(svg(big, big, tile_edge() + boat()))
        glyph_png = Path(tmp) / 'glyph.png'
        # ImageMagick reads SVG sizes at 96 dpi; force the exact size anyway.
        magick('-background', 'none', '-density', 96, glyph_svg,
               '-resize', f'{big}x{big}!', glyph_png)
        flat = Path(tmp) / 'flat.png'
        magick('-size', f'{big}x{big}', 'xc:none',
               '(', '-size', f'{tile_px}x{tile_px}', f'gradient:{TILE_TOP}-{TILE_BOTTOM}',
               '(', '-size', f'{tile_px}x{tile_px // 2}', 'gradient:#FFFFFF1A-#FFFFFF00',
               '-background', 'none', '-extent', f'{tile_px}x{tile_px}', ')',
               '-compose', 'Over', '-composite',
               '(', '-size', f'{tile_px}x{tile_px}', 'xc:black', '-fill', 'white',
               '-draw', f'roundrectangle 0,0 {tile_px - 1},{tile_px - 1} {radius},{radius}', ')',
               '-alpha', 'off', '-compose', 'CopyOpacity', '-composite', ')',
               '-geometry', f'+{offset}+{offset}', '-compose', 'Over', '-composite',
               glyph_png, '-compose', 'Over', '-composite',
               '-filter', 'Box', '-resize', f'{canvas}x{canvas}', flat)
        # The soft shadow under the tile, as Sail's icon has.
        magick(flat, '(', '+clone', '-fill', 'black', '-colorize', '100',
               '-channel', 'A', '-evaluate', 'multiply', 0.30, '+channel',
               '-blur', '0x12', ')', '-geometry', '+0+10',
               '-compose', 'DstOver', '-composite', out)


def icns(master, out):
    with tempfile.TemporaryDirectory() as tmp:
        iconset = Path(tmp) / 'Keel.iconset'
        iconset.mkdir()
        for points in (16, 32, 128, 256, 512):
            for factor in (1, 2):
                pixels = points * factor
                suffix = '' if factor == 1 else '@2x'
                magick(master, '-filter', 'Lanczos', '-resize', f'{pixels}x{pixels}',
                       iconset / f'icon_{points}x{points}{suffix}.png')
        subprocess.run(['iconutil', '-c', 'icns', iconset, '-o', out], check=True)


def main():
    if not shutil.which('magick'):
        raise SystemExit('ImageMagick is missing: brew install imagemagick')
    (OUT / 'keel-mark.svg').write_text(mark_svg())
    (OUT / 'keel-icon.svg').write_text(icon_svg())
    master = OUT / 'keel-icon-1024.png'
    icon_png(master)
    icns(master, OUT / 'Keel.icns')
    print(f'Wrote the mark, the icon and Keel.icns in {OUT}')


if __name__ == '__main__':
    main()
