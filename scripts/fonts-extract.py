#!/usr/bin/env python3
"""Builds the script faces of Fluxer's `--font-sans` chain from @fluxer/fonts (fontTools).

    scripts/fonts-extract.py <fluxer>/packages/fonts/files [out dir, default assets/fonts]

Each `FluxerSans{script}-{weight}` is merged from its woff2 subsets into one TTF. They ship in
a `fonts` folder beside the binary rather than built in (`theme::app_fonts` maps them).
"""
import glob, io, os, sys
from fontTools.ttLib import TTFont
from fontTools.merge import Merger

SCRIPTS = ['KR', 'SC', 'TC', 'JP', 'Arabic', 'Hebrew', 'Devanagari', 'ThaiLooped']
WEIGHTS = ['Regular', 'Medium', 'SemiBold', 'Bold']
src = sys.argv[1]
out = sys.argv[2] if len(sys.argv) > 2 else os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'assets', 'fonts')


def ttf(path):
    t = TTFont(path)
    t.flavor = None
    b = io.BytesIO()
    t.save(b)
    b.seek(0)
    return b


for script in SCRIPTS:
    for w in WEIGHTS:
        name = f'FluxerSans{script}-{w}'
        parts = sorted(glob.glob(f'{src}/FluxerSans{script}/{name}.woff2') + glob.glob(f'{src}/FluxerSans{script}/{name}.*.woff2'))
        if not parts:
            sys.exit(f'{name}: no woff2')
        font = Merger().merge([ttf(p) for p in parts]) if len(parts) > 1 else TTFont(ttf(parts[0]))
        font.save(os.path.join(out, f'{name}.ttf'))
        print(name, len(parts), 'parts', len(font.getGlyphOrder()), 'glyphs', flush=True)
