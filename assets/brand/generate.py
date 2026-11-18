#!/usr/bin/env python3
"""Generates Keel's mark and app icon from the paper boat glyph.

The geometry in this file is the single source of truth for the logo. The
outputs are committed next to this file, so a normal build never runs this
script. Run it again only after changing the logo:

    python3 -B assets/brand/generate.py

Requires Xcode (for `actool`) and `sips`, both on macOS.

The logo is a paper boat folded from a page of code: the boat stands for
the name, the coloured lines are Keel's syntax colours. The tile uses the
blues of Sail, Manuel's browser, so the two apps look like one family:
Sail is the sail, Keel is the boat.

What it writes:
- `keel-mark.svg`, the boat in one colour, drawn inside Keel;
- `Keel.icon`, the app icon in Icon Composer's format: the boat in glass,
  its lines of code and the water, in layers on a gradient, with a light
  and a dark version. macOS 26 and later draw it as their own icons, in
  Liquid Glass; an icon in the old format gets shrunk into a dark square;
- `Assets.car` and `Keel.icns`, the icon compiled by `actool`: macOS 26 and
  later read the first, older versions the second;
- `keel-icon-1024.png`, the icon as a picture, for documents.
"""

import json
import shutil
import subprocess
import tempfile
from pathlib import Path

OUT = Path(__file__).resolve().parent
CANVAS = 1024

# Sail's gradient for the light version, from the top down; the dark one
# is Keel's midnight, so the icon stays blue in dark mode instead of black.
LIGHT_FILL = ['display-p3:0.05490,0.11373,0.36863,1.00000',
              'display-p3:0.33333,0.40784,0.85098,1.00000']
DARK_FILL = ['srgb:0.06275,0.10588,0.27059,1.00000',
             'srgb:0.13333,0.21961,0.52941,1.00000']

# The boat, centred on the 1024 canvas: a sail and a hull, each folded in
# two, with the light coming from the left.
CX = 512
SAIL_TOP, SAIL_BASE, SAIL_HALF = 214, 556, 170
HULL_TOP, HULL_BOTTOM = 556, 772
HULL_HALF_TOP, HULL_HALF_BOTTOM = 372, 232

FACETS = [
    # Sail, lit and shaded.
    (f'M{CX} {SAIL_TOP} L {CX - SAIL_HALF} {SAIL_BASE} H {CX} Z', '#FFFFFF'),
    (f'M{CX} {SAIL_TOP} L {CX} {SAIL_BASE} H {CX + SAIL_HALF} Z', '#C7D6F6'),
    # Hull, lit and shaded.
    (f'M{CX - HULL_HALF_TOP} {HULL_TOP} H {CX} V {HULL_BOTTOM} '
     f'H {CX - HULL_HALF_BOTTOM} Z', '#EEF3FD'),
    (f'M{CX} {HULL_TOP} H {CX + HULL_HALF_TOP} L {CX + HULL_HALF_BOTTOM} '
     f'{HULL_BOTTOM} H {CX} Z', '#9FB6E8'),
]

PINK, BLUE, GREEN, VIOLET = '#FF4F9A', '#2F74F5', '#14A37F', '#7C4DF2'
SHADE = '#2A3F72'
LINE_HEIGHT = 22
# The lines of code printed on the lit side, (x, y, width, colour). They
# end at the fold and stay inside the paper, so nothing needs clipping.
CODE = [
    (CX - 86, 384, 86, PINK), (CX - 120, 428, 52, BLUE), (CX - 60, 428, 60, GREEN),
    (CX - 150, 472, 150, VIOLET), (CX - 176, 516, 70, PINK), (CX - 96, 516, 96, BLUE),
    (CX - 300, 600, 130, BLUE), (CX - 160, 600, 112, PINK),
    (CX - 250, 646, 210, GREEN),
    (CX - 236, 692, 80, VIOLET), (CX - 146, 692, 146, BLUE),
    (CX - 200, 738, 112, PINK),
]
# The shaded side's lines, as print seen through the fold.
SHADED_CODE = [(CX + 14, 384, 46), (CX + 14, 428, 96), (CX + 14, 472, 74),
               (CX + 14, 516, 118), (CX + 14, 600, 160), (CX + 14, 646, 230),
               (CX + 14, 692, 128), (CX + 14, 738, 150)]

# Two strokes of water under the boat: (left, y, right, opacity).
WATER = [(300, 812, 724, 0.9), (380, 864, 644, 0.55)]
WATER_HEIGHT = 20


def svg(width, height, body, view_box=f'0 0 {CANVAS} {CANVAS}'):
    return (f'<svg width="{width}" height="{height}" viewBox="{view_box}" '
            f'fill="none" xmlns="http://www.w3.org/2000/svg">{body}</svg>\n')


def bars(items):
    return ''.join(
        f'<rect x="{x}" y="{y}" width="{width}" height="{LINE_HEIGHT}" '
        f'rx="{LINE_HEIGHT / 2}" fill="{colour}"/>'
        for x, y, width, colour in items)


def boat_layer():
    return svg(CANVAS, CANVAS,
               ''.join(f'<path d="{path}" fill="{colour}"/>' for path, colour in FACETS))


def code_layer():
    shaded = [(x, y, width, SHADE) for x, y, width in SHADED_CODE]
    return svg(CANVAS, CANVAS, bars(CODE) + bars(shaded))


def water_layer():
    half = WATER_HEIGHT / 2
    return svg(CANVAS, CANVAS, ''.join(
        f'<rect x="{left}" y="{y}" width="{right - left}" height="{WATER_HEIGHT}" '
        f'rx="{half}" fill="#FFFFFF" fill-opacity="{opacity}"/>'
        for left, y, right, opacity in WATER))


def mark_svg():
    """The boat's outline in one colour, for use inside Keel: sail and hull
    with a gap between them, so it reads at 16 px. It keeps the first
    proportions, which the toolbar and the welcome page are drawn for."""
    sail_top, sail_base, sail_half = 220, 548, 160
    hull_top, hull_bottom, half_top, half_bottom = 548, 760, 362, 226
    gap = 28
    sail = f'M512 {sail_top} L {512 - sail_half} {sail_base} H {512 + sail_half} Z'
    hull = (f'M{512 - half_top} {hull_top + gap} H {512 + half_top} '
            f'L {512 + half_bottom} {hull_bottom} H {512 - half_bottom} Z')
    body = f'<path d="{sail}" fill="currentColor"/><path d="{hull}" fill="currentColor"/>'
    return svg(24, 24, body, view_box='130 108 764 764')


def icon_document():
    """Icon Composer's description of the icon: the background, then the
    groups from the front: the code, the boat in glass, the water."""
    return {
        'fill-specializations': [
            {'value': {'linear-gradient': LIGHT_FILL}},
            {'appearance': 'dark', 'value': {'linear-gradient': DARK_FILL}},
        ],
        'groups': [
            {
                'layers': [{'image-name': 'code.svg', 'name': 'code', 'glass': False}],
                'shadow': {'kind': 'neutral', 'opacity': 0},
                'translucency': {'enabled': False, 'value': 0},
            },
            {
                'layers': [{'image-name': 'boat.svg', 'name': 'boat', 'glass': True}],
                'shadow': {'kind': 'neutral', 'opacity': 0.35},
                'specular': 'inside',
                'translucency': {'enabled': True, 'value': 0.2},
            },
            {
                'layers': [{'image-name': 'water.svg', 'name': 'water', 'glass': True,
                            'opacity': 0.7}],
                'shadow': {'kind': 'neutral', 'opacity': 0.1},
                'translucency': {'enabled': True, 'value': 0.5},
            },
        ],
        'supported-platforms': {'squares': ['macOS']},
    }


def write_icon(folder):
    shutil.rmtree(folder, ignore_errors=True)
    assets = folder / 'Assets'
    assets.mkdir(parents=True)
    (assets / 'boat.svg').write_text(boat_layer())
    (assets / 'code.svg').write_text(code_layer())
    (assets / 'water.svg').write_text(water_layer())
    (folder / 'icon.json').write_text(json.dumps(icon_document(), indent=2) + '\n')


def compile_icon(icon, out):
    """`actool`, as Xcode runs it for an app: Assets.car for macOS 26 and
    later, Keel.icns for the earlier versions."""
    with tempfile.TemporaryDirectory() as tmp:
        subprocess.run(
            ['xcrun', 'actool', icon, '--compile', tmp,
             '--output-format', 'human-readable-text', '--errors', '--warnings',
             '--output-partial-info-plist', Path(tmp) / 'partial.plist',
             '--app-icon', 'Keel', '--include-all-app-icons',
             '--enable-on-demand-resources', 'NO', '--development-region', 'it',
             '--target-device', 'mac', '--minimum-deployment-target', '13.0',
             '--platform', 'macosx'],
            check=True, stdout=subprocess.DEVNULL)
        for name in ('Assets.car', 'Keel.icns'):
            shutil.copy(Path(tmp) / name, out / name)


def main():
    if not shutil.which('xcrun'):
        raise SystemExit('Xcode is missing: its actool compiles the icon')
    (OUT / 'keel-mark.svg').write_text(mark_svg())
    write_icon(OUT / 'Keel.icon')
    compile_icon(OUT / 'Keel.icon', OUT)
    subprocess.run(['sips', '-s', 'format', 'png', '-z', str(CANVAS), str(CANVAS),
                    OUT / 'Keel.icns', '--out', OUT / 'keel-icon-1024.png'],
                   check=True, stdout=subprocess.DEVNULL)
    print(f'Wrote the mark, Keel.icon, Assets.car and Keel.icns in {OUT}')


if __name__ == '__main__':
    main()
