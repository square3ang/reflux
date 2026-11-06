#!/usr/bin/env python3
"""Copies the Fluxer translations Reflux uses into assets/i18n/<locale>.json.

Fluxer's catalogs (lingui .po, ~2.5 MB per locale) are keyed by the English source text. Reflux
looks strings up by that same text (`t("Welcome back")`), so this keeps the entries whose English
text appears as a string literal anywhere in crates/reflux/src (labels in tables and `if` arms
included). Re-run it after adding UI strings.

Usage: scripts/i18n-extract.py [/path/to/fluxer]   (default /tmp/opencode/fluxer)
"""
import json
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
FLUXER = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else "/tmp/opencode/fluxer")
LOCALES = FLUXER / "fluxer_app/src/features/i18n/locales"
OUT = ROOT / "assets/i18n"

# Plain Rust string literals (raw strings aren't UI text here).
CALL = re.compile(r'"((?:[^"\\\n]|\\.)*)"')


def unescape_rust(s):
    # Only the escapes UI text uses; anything fancier simply won't match a catalog key.
    return s.replace('\\"', '"').replace("\\n", "\n").replace("\\'", "'").replace("\\\\", "\\")


def used_keys():
    keys = set()
    for f in (ROOT / "crates/reflux/src").rglob("*.rs"):
        for m in CALL.finditer(f.read_text()):
            keys.add(unescape_rust(m.group(1)))
    return keys


def po_string(lines):
    out = []
    for line in lines:
        s = line[line.index('"') + 1 : line.rindex('"')]
        out.append(s.replace('\\"', '"').replace("\\n", "\n").replace("\\t", "\t").replace("\\\\", "\\"))
    return "".join(out)


def parse_po(path):
    """{msgid: msgstr}; the first translation wins when msgctxt repeats an id."""
    entries, cur, field = {}, {}, None

    def flush():
        if "msgid" in cur and cur["msgid"]:
            mid, mstr = po_string(cur["msgid"]), po_string(cur.get("msgstr", []))
            if mid and mstr and mid not in entries:
                entries[mid] = mstr
        cur.clear()

    for line in path.read_text(encoding="utf-8").splitlines():
        if not line.strip():
            flush()
            field = None
        elif line.startswith("#"):
            continue
        elif line.startswith('"') and field:
            cur[field].append(line)
        else:
            field = line.split(" ", 1)[0]
            cur[field] = [line]
    flush()
    return entries


def main():
    keys = used_keys()
    OUT.mkdir(parents=True, exist_ok=True)
    for d in sorted(LOCALES.iterdir()):
        po = d / "messages.po"
        if not po.exists() or d.name == "en-US":
            continue
        cat = parse_po(po)
        subset = {k: cat[k] for k in sorted(keys) if k in cat and cat[k] != k}
        (OUT / f"{d.name}.json").write_text(json.dumps(subset, ensure_ascii=False, indent=0, sort_keys=True) + "\n")
    ko = parse_po(LOCALES / "ko/messages.po")
    print(f"{sum(k in ko for k in keys)} of {len(keys)} string literals have Fluxer translations")


if __name__ == "__main__":
    main()
