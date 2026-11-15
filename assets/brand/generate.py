#!/usr/bin/env python3
"""Generates Keel's mark and app icon from the keel glyph.

The glyph geometry in this file is the single source of truth for the logo.
The outputs are committed next to this file, so a normal build never runs
this script. Run it again only after changing the logo:

    python3 assets/brand/generate.py

Requires ImageMagick (`magick`) and `iconutil` (part of macOS).

The glyph is the keel of a sailing boat seen from the bow: the hull, the
fin and the bulb. The tile uses the midnight blues of Sail, Manuel's
browser, so the two apps look like one family.
"""

import shutil
import subprocess
import tempfile
from pathlib import Path

OUT = Path(__file__).resolve().parent

# Apple's icon grid: a 1024 canvas with an 824 tile whose corners are 185.4.
APPLE_MARGIN = 100
RADIUS_RATIO = 185.4 / 824
# Sail's "blu-notte" palette: gradient top and bottom.
TILE_TOP = '#1E2B47'
TILE_BOTTOM = '#070B14'
GLYPH = '#FFFFFF'

# The glyph on a 1024 canvas, centred on 512.
DECK_Y = 308          # the deck line, the top of the hull
HULL_LEFT = 232
HULL_RIGHT = 792
HULL_BOTTOM = 468     # the lowest point of the hull
FIN_TOP = 450         # inside the hull, so the two join cleanly
FIN_BOTTOM = 640
FIN_TOP_HALF = 34     # the fin narrows towards the bulb
FIN_BOTTOM_HALF = 26
BULB_Y = 664
BULB_RX = 74
BULB_RY = 54


def fmt(value):
    return f'{value:.2f}'.rstrip('0').rstrip('.')


def glyph_paths(scale=1.0, dx=0.0, dy=0.0):
    """The glyph as SVG elements, scaled and moved."""
    def x(value):
        return fmt(value * scale + dx)

    def y(value):
        return fmt(value * scale + dy)

    centre = 512
    depth = HULL_BOTTOM - DECK_Y
    half = (HULL_RIGHT - HULL_LEFT) / 2
    hull = (
        f'M{x(HULL_LEFT)} {y(DECK_Y)} H{x(HULL_RIGHT)} '
        f'C{x(HULL_RIGHT)} {y(DECK_Y + depth * 0.56)} {x(centre + half * 0.56)} {y(HULL_BOTTOM)} '
        f'{x(centre)} {y(HULL_BOTTOM)} '
        f'C{x(centre - half * 0.56)} {y(HULL_BOTTOM)} {x(HULL_LEFT)} {y(DECK_Y + depth * 0.56)} '
        f'{x(HULL_LEFT)} {y(DECK_Y)} Z'
    )
    fin = (
        f'M{x(centre - FIN_TOP_HALF)} {y(FIN_TOP)} H{x(centre + FIN_TOP_HALF)} '
        f'L{x(centre + FIN_BOTTOM_HALF)} {y(FIN_BOTTOM)} H{x(centre - FIN_BOTTOM_HALF)} Z'
    )
    return [
        ('path', {'d': hull}),
        ('path', {'d': fin}),
        ('ellipse', {'cx': x(centre), 'cy': y(BULB_Y), 'rx': fmt(BULB_RX * scale),
                     'ry': fmt(BULB_RY * scale)}),
    ]


def elements(fill, **transform):
    return ''.join(
        f'<{tag} ' + ' '.join(f'{key}="{value}"' for key, value in attributes.items())
        + f' fill="{fill}"/>'
        for tag, attributes in glyph_paths(**transform)
    )


def svg(width, height, body, view_box=None):
    view_box = view_box or f'0 0 {width} {height}'
    return (f'<svg width="{width}" height="{height}" viewBox="{view_box}" '
            f'fill="none" xmlns="http://www.w3.org/2000/svg">{body}</svg>\n')


def mark_svg():
    """The glyph alone, in the text colour, for use inside Keel."""
    return svg(24, 24, elements('currentColor'), view_box='202 202 620 620')


def icon_svg():
    """The vector master: tile and glyph on Apple's 1024 grid."""
    size = 1024 - 2 * APPLE_MARGIN
    scale = size / 1024
    return svg(1024, 1024,
               '<defs><linearGradient id="tile" x1="0" y1="0" x2="0" y2="1">'
               f'<stop offset="0" stop-color="{TILE_TOP}"/>'
               f'<stop offset="1" stop-color="{TILE_BOTTOM}"/></linearGradient></defs>'
               f'<rect x="{APPLE_MARGIN}" y="{APPLE_MARGIN}" width="{size}" height="{size}" '
               f'rx="{fmt(size * RADIUS_RATIO)}" fill="url(#tile)"/>'
               + elements(GLYPH, scale=scale, dx=APPLE_MARGIN, dy=APPLE_MARGIN))


def magick(*args):
    subprocess.run(['magick', *map(str, args)], check=True)


def icon_png(out, supersample=4):
    """The 1024 icon. ImageMagick's SVG renderer ignores gradients, so the
    tile is drawn with ImageMagick primitives and the glyph on top."""
    canvas = 1024
    size = canvas - 2 * APPLE_MARGIN
    big = canvas * supersample
    tile_px = size * supersample
    radius = fmt(tile_px * RADIUS_RATIO)
    scale = size / 1024 * supersample
    offset = APPLE_MARGIN * supersample
    glyph = svg(big, big, elements(GLYPH, scale=scale, dx=offset, dy=offset))
    with tempfile.TemporaryDirectory() as tmp:
        glyph_svg = Path(tmp) / 'glyph.svg'
        glyph_svg.write_text(glyph)
        glyph_png = Path(tmp) / 'glyph.png'
        # ImageMagick reads SVG sizes at 96 dpi; force the exact size anyway.
        magick('-background', 'none', '-density', 96, glyph_svg,
               '-resize', f'{big}x{big}!', glyph_png)
        flat = Path(tmp) / 'flat.png'
        magick('-size', f'{big}x{big}', 'xc:none',
               '(', '-size', f'{tile_px}x{tile_px}', f'gradient:{TILE_TOP}-{TILE_BOTTOM}',
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
