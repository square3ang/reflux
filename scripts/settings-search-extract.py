#!/usr/bin/env python3
"""Generates crates/reflux/src/settings/search_index.rs from Fluxer's settings search index
(`settings_utils/search_index/*.ts` + `section_registry/*.ts`): every searchable item's tab,
label, keywords and description, as English text (Reflux translates it with `t`).

Usage: scripts/settings-search-extract.py [/path/to/fluxer]   (default /tmp/opencode/fluxer)
"""
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
FLUXER = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else "/tmp/opencode/fluxer")
SRC = FLUXER / "fluxer_app/src"
UTILS = SRC / "features/user/components/settings_utils"
OUT = ROOT / "crates/reflux/src/settings/search_index.rs"

STR = r"'((?:[^'\\]|\\.)*)'|\"((?:[^\"\\]|\\.)*)\"|`((?:[^`\\]|\\.)*)`"
# `const NAME = msg({ message: '…'` or msg`…`
DESC = re.compile(r"(?:export\s+)?const\s+(\w+)\s*=\s*msg\(\s*\{\s*(?:id:\s*(?:" + STR + r")\s*,\s*)?message:\s*(?:" + STR + r")", re.S)
DESC_TPL = re.compile(r"(?:export\s+)?const\s+(\w+)\s*=\s*msg`((?:[^`\\]|\\.)*)`")
ALIAS = re.compile(r"\b(\w+)\s+as\s+(\w+)\b")


def unescape(s):
    return s.replace("\\'", "'").replace('\\"', '"').replace("\\`", "`").replace("\\\\", "\\")


def descriptors():
    out = {}
    for f in SRC.rglob("*.ts*"):
        txt = f.read_text(encoding="utf-8", errors="ignore")
        if "msg" not in txt:
            continue
        for m in DESC.finditer(txt):
            g = [x for x in m.groups()[1:] if x is not None]
            out.setdefault(m.group(1), unescape(g[-1]))
        for m in DESC_TPL.finditer(txt):
            out.setdefault(m.group(1), unescape(m.group(2)))
    return out


def objects(txt):
    """Top-level `{ … }` items of the exported arrays."""
    i = txt.find("= [")
    while i != -1:
        depth, start = 0, None
        j = i + 3
        while j < len(txt):
            c = txt[j]
            if c in "'\"`":
                q = c
                j += 1
                while txt[j] != q:
                    j += 2 if txt[j] == "\\" else 1
            elif c == "{":
                if depth == 0:
                    start = j
                depth += 1
            elif c == "}":
                depth -= 1
                if depth == 0:
                    yield txt[start : j + 1]
            elif c == "]" and depth == 0:
                break
            j += 1
        i = txt.find("= [", j)


def resolve(expr, names, aliases):
    expr = expr.strip()
    m = re.fullmatch(STR, expr)
    if m:
        return unescape(next(x for x in m.groups() if x is not None))
    m = re.fullmatch(r"msg`((?:[^`\\]|\\.)*)`", expr)
    if m:
        return unescape(m.group(1))
    m = re.fullmatch(r"msg\(\s*\{.*?message:\s*(?:" + STR + r").*\}\s*\)", expr, re.S)
    if m:
        return unescape(next(x for x in m.groups() if x is not None))
    name = aliases.get(expr, expr)
    return names.get(name)


def field(obj, key):
    m = re.search(r"\n\t\t" + key + r":\s*", obj)
    if not m:
        return None
    j, depth = m.end(), 0
    k = j
    while k < len(obj):
        c = obj[k]
        if c in "'\"`":
            q = c
            k += 1
            while obj[k] != q:
                k += 2 if obj[k] == "\\" else 1
        elif c in "([{":
            depth += 1
        elif c in ")]}":
            if depth == 0:
                break
            depth -= 1
        elif c == "," and depth == 0:
            break
        k += 1
    return obj[j:k].strip()


def split_list(s):
    s = s.strip()[1:-1]
    parts, depth, cur, k = [], 0, "", 0
    while k < len(s):
        c = s[k]
        if c in "'\"`":
            q, st = c, k
            k += 1
            while s[k] != q:
                k += 2 if s[k] == "\\" else 1
            cur += s[st : k + 1]
        elif c in "([{":
            depth += 1
            cur += c
        elif c in ")]}":
            depth -= 1
            cur += c
        elif c == "," and depth == 0:
            parts.append(cur)
            cur = ""
        else:
            cur += c
        k += 1
    parts.append(cur)
    return [p.strip() for p in parts if p.strip()]


def rs(s):
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"').replace("\n", "\\n") + '"'


def main():
    names = descriptors()
    items, missing, added = [], set(), []
    files = sorted((UTILS / "section_registry").glob("*Sections.ts")) + sorted((UTILS / "search_index").glob("*Index.ts"))
    for f in files:
        txt = f.read_text(encoding="utf-8")
        aliases = {b: a for a, b in ALIAS.findall(txt)}
        for obj in objects(txt):
            tab = field(obj, "tabType")
            label = field(obj, "label")
            if not tab or not label:
                continue
            tab = tab.strip("'\"")
            lab = resolve(label, names, aliases)
            kws = []
            for k in split_list(field(obj, "keywords") or "[]"):
                v = resolve(k, names, aliases)
                if v is None:
                    missing.add(k)
                else:
                    kws.append(v)
            d = field(obj, "description")
            desc = resolve(d, names, aliases) if d else None
            if lab is None:
                missing.add(label)
                continue
            section = (field(obj, "id") if "Sections" in f.name else field(obj, "sectionId") or "").strip("'\"")
            ident = (field(obj, "id") or "").strip("'\"")
            if "Sections" in f.name:
                ident = f"section-{tab}-{ident}"
            # `getSettingsAudience`.
            advanced = (field(obj, "isAdvanced") == "true") or field(obj, "audience") == "'advanced'" or ident.startswith("advanced-")
            items.append((ident, tab, section, lab, kws, desc or "", advanced))
            at = (field(obj, "addedAt") or "").strip("'\"")
            if at:
                added.append((ident, at))
    if missing:
        print("unresolved:", sorted(missing), file=sys.stderr)
    lines = [
        "//! Generated by scripts/settings-search-extract.py from Fluxer's settings search index",
        "//! (`settings_utils/search_index/*.ts`, `section_registry/*.ts`). Don't edit by hand.",
        "",
        "/// (id, tab, section id, label, keywords, description, advanced audience), English.",
        "#[allow(clippy::type_complexity)]",
        "pub(super) const ITEMS: &[(&str, &str, &str, &str, &[&str], &str, bool)] = &[",
    ]
    for ident, tab, sec, lab, kws, desc, adv in items:
        lines.append(f"    ({rs(ident)}, {rs(tab)}, {rs(sec)}, {rs(lab)}, &[{', '.join(rs(k) for k in kws)}], {rs(desc)}, {'true' if adv else 'false'}),")
    lines.append("];")
    lines += ["", "/// (id, `addedAt`) for the items that have one, for \"New settings\" (`isSettingsItemNew`).", "pub(super) const ADDED: &[(&str, &str)] = &["]
    lines += [f"    ({rs(i)}, {rs(a)})," for i, a in added]
    lines.append("];")
    OUT.write_text("\n".join(lines) + "\n")
    print(f"{len(items)} items, {len(missing)} unresolved")


if __name__ == "__main__":
    main()
