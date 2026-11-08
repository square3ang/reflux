#!/usr/bin/env python3
"""Builds assets/fonts from @fluxer/fonts' woff2 subsets (fontTools).

    scripts/fonts-extract.py <fluxer>/packages/fonts/files

- FluxerSansKR-{weight}.ttf: the KR subsets merged (shipped in a `fonts` folder, not built in).
- FluxerSansSC-Symbols-{weight}.ttf: Fluxer Sans SC without ideographs, kana, Hangul and what
  Fluxer Sans covers (blocks, box drawing, arrows, shapes, full-width punctuation).
"""
import glob, io, os, sys
from fontTools.ttLib import TTFont
from fontTools.merge import Merger
from fontTools import subset

WEIGHTS = ['Regular', 'Medium', 'SemiBold', 'Bold']
src = sys.argv[1]
out = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'assets', 'fonts')


def ttf(path):
    t = TTFont(path)
    t.flavor = None
    b = io.BytesIO()
    t.save(b)
    b.seek(0)
    return b


def cjk(c):
    return (0x1100 <= c <= 0x11FF or 0x2E80 <= c <= 0x2FDF or 0x3040 <= c <= 0x318F or 0x31A0 <= c <= 0x31FF
            or 0x3400 <= c <= 0x4DBF or 0x4E00 <= c <= 0x9FFF or 0xA960 <= c <= 0xA97F or 0xAC00 <= c <= 0xD7FF
            or 0xF900 <= c <= 0xFAFF or 0xFF65 <= c <= 0xFFDC or 0x20000 <= c <= 0x3FFFF)


own = set(TTFont(os.path.join(out, 'FluxerSans-Regular.ttf')).getBestCmap())
for w in WEIGHTS:
    files = [ttf(f) for f in sorted(glob.glob(f'{src}/FluxerSansKR/FluxerSansKR-{w}.*.woff2'))]
    Merger().merge(files).save(os.path.join(out, f'FluxerSansKR-{w}.ttf'))

    keep, files = set(), []
    for f in sorted(glob.glob(f'{src}/FluxerSansSC/FluxerSansSC-{w}.*.woff2')):
        cps = [c for c in TTFont(f).getBestCmap() if not cjk(c) and c not in own]
        if cps:
            keep.update(cps)
            files.append(ttf(f))
    b = io.BytesIO()
    (Merger().merge(files) if len(files) > 1 else TTFont(files[0])).save(b)
    b.seek(0)
    font = TTFont(b)
    o = subset.Options()
    o.layout_features, o.name_IDs, o.notdef_outline, o.hinting = ['*'], ['*'], True, True
    s = subset.Subsetter(o)
    s.populate(unicodes=sorted(keep))
    s.subset(font)
    font.save(os.path.join(out, f'FluxerSansSC-Symbols-{w}.ttf'))
    print(w, len(keep), 'symbols')
