#!/usr/bin/env python3
"""UCD 16.0.0 -> crates/torajs-regex/src/ucd_tables.rs generator.

RFC 20260711-regex-property-escapes chunk A. Reads the UCD data files
under ./data (downloaded from unicode.org/Public/16.0.0/ucd/) and
emits one CODEGEN Rust file with sorted, merged UPropRange tables for:

- General_Category values (38: the 30 leaf categories + L/LC/M/N/P/S/Z/C composites)
- Script values (canonical long names)
- Script_Extensions values (same name domain, superset ranges)
- The ES section 22.2.1 binary property whitelist
- alias -> canonical tables (PropertyValueAliases.txt), sorted for binary search

Usage: python3 gen.py > ../../crates/torajs-regex/src/ucd_tables.rs
"""

import sys
from collections import defaultdict
from pathlib import Path

DATA = Path(__file__).parent / "data"

# ES 2024 section 22.2.1 lone binary properties (spec table 65).
ES_BINARY = [
    "ASCII", "ASCII_Hex_Digit", "Alphabetic", "Any", "Assigned",
    "Bidi_Control", "Bidi_Mirrored", "Case_Ignorable", "Cased",
    "Changes_When_Casefolded", "Changes_When_Casemapped",
    "Changes_When_Lowercased", "Changes_When_NFKC_Casefolded",
    "Changes_When_Titlecased", "Changes_When_Uppercased", "Dash",
    "Default_Ignorable_Code_Point", "Deprecated", "Diacritic", "Emoji",
    "Emoji_Component", "Emoji_Modifier", "Emoji_Modifier_Base",
    "Emoji_Presentation", "Extended_Pictographic", "Extender",
    "Grapheme_Base", "Grapheme_Extend", "Hex_Digit",
    "IDS_Binary_Operator", "IDS_Trinary_Operator", "ID_Continue",
    "ID_Start", "Ideographic", "Join_Control", "Logical_Order_Exception",
    "Lowercase", "Math", "Noncharacter_Code_Point", "Pattern_Syntax",
    "Pattern_White_Space", "Quotation_Mark", "Radical",
    "Regional_Indicator", "Sentence_Terminal", "Soft_Dotted",
    "Terminal_Punctuation", "Unified_Ideograph", "Uppercase",
    "Variation_Selector", "White_Space", "XID_Continue", "XID_Start",
]

GC_COMPOSITES = {
    "L": ["Lu", "Ll", "Lt", "Lm", "Lo"],
    "LC": ["Lu", "Ll", "Lt"],
    "M": ["Mn", "Mc", "Me"],
    "N": ["Nd", "Nl", "No"],
    "P": ["Pc", "Pd", "Ps", "Pe", "Pi", "Pf", "Po"],
    "S": ["Sm", "Sc", "Sk", "So"],
    "Z": ["Zs", "Zl", "Zp"],
    "C": ["Cc", "Cf", "Co", "Cs", "Cn"],
}

MAX_CP = 0x10FFFF


def parse_ranges(path, value_field=1):
    """Parse `XXXX[..YYYY] ; Value [; ...] # comment` lines into
    {value: [(lo, hi)]}. value_field picks the ;-separated column."""
    out = defaultdict(list)
    for line in open(path, encoding="utf-8"):
        line = line.split("#", 1)[0].strip()
        if not line:
            continue
        parts = [p.strip() for p in line.split(";")]
        if len(parts) <= value_field:
            continue
        cps = parts[0]
        if ".." in cps:
            lo, hi = (int(x, 16) for x in cps.split(".."))
        else:
            lo = hi = int(cps, 16)
        out[parts[value_field]].append((lo, hi))
    return out


def parse_scx(path):
    """ScriptExtensions.txt: value column is a space-separated list of
    script SHORT codes; each listed script gets the range."""
    out = defaultdict(list)
    for line in open(path, encoding="utf-8"):
        line = line.split("#", 1)[0].strip()
        if not line:
            continue
        cps, codes = (p.strip() for p in line.split(";", 1))
        if ".." in cps:
            lo, hi = (int(x, 16) for x in cps.split(".."))
        else:
            lo = hi = int(cps, 16)
        for code in codes.split():
            out[code].append((lo, hi))
    return out


def merge(ranges):
    """Sort + coalesce adjacent/overlapping ranges."""
    if not ranges:
        return []
    ranges = sorted(ranges)
    out = [list(ranges[0])]
    for lo, hi in ranges[1:]:
        if lo <= out[-1][1] + 1:
            out[-1][1] = max(out[-1][1], hi)
        else:
            out.append([lo, hi])
    return [tuple(r) for r in out]


def complement(ranges):
    """[0, MAX_CP] minus ranges (for Cn = unassigned, and Assigned)."""
    out = []
    cur = 0
    for lo, hi in ranges:
        if lo > cur:
            out.append((cur, lo - 1))
        cur = hi + 1
    if cur <= MAX_CP:
        out.append((cur, MAX_CP))
    return out


def parse_binary_aliases(path, es_binary):
    """PropertyAliases.txt rows: `AHex ; ASCII_Hex_Digit [; more]`.
    Canonical is the LONG name (column 2); rows whose long name is in
    the ES binary whitelist contribute every other spelling as an
    alias (ES 22.2.1 accepts binary property names AND aliases)."""
    out = {}
    for line in open(path, encoding="utf-8"):
        line = line.split("#", 1)[0].strip()
        if not line:
            continue
        parts = [p.strip() for p in line.split(";")]
        if len(parts) < 2 or parts[1] not in es_binary:
            continue
        long = parts[1]
        for name in [parts[0]] + parts[2:]:
            if name and name != long:
                out[name] = long
    return out


def parse_aliases(path):
    """PropertyValueAliases.txt rows for gc and sc:
    `gc ; Lu ; Uppercase_Letter [; more]` -> every name maps to a
    canonical spelling. For gc the canonical is the SHORT name (Lu);
    for sc the canonical is the LONG name (Adlam, column 3)."""
    gc_alias = {}
    sc_alias = {}
    sc_short_to_long = {}
    for line in open(path, encoding="utf-8"):
        line = line.split("#", 1)[0].strip()
        if not line:
            continue
        parts = [p.strip() for p in line.split(";")]
        if parts[0] == "gc":
            short = parts[1]
            for name in parts[2:]:
                if name and name != short:
                    gc_alias[name] = short
        elif parts[0] == "sc":
            short, long = parts[1], parts[2]
            sc_short_to_long[short] = long
            if short != long:
                sc_alias[short] = long
            for name in parts[3:]:
                if name and name != long:
                    sc_alias[name] = long
    return gc_alias, sc_alias, sc_short_to_long


def rs_ranges(name, ranges):
    body = "".join(
        f"    UPropRange {{ lo: 0x{lo:04X}, hi: 0x{hi:04X} }},\n" for lo, hi in ranges
    )
    return f"static {name}: &[UPropRange] = &[\n{body}];\n"


def sanitize(value):
    return value.upper().replace("-", "_")


def main():
    gc = parse_ranges(DATA / "extracted" / "DerivedGeneralCategory.txt")
    scripts = parse_ranges(DATA / "Scripts.txt")
    scx_by_short = parse_scx(DATA / "ScriptExtensions.txt")
    gc_alias, sc_alias, sc_short_to_long = parse_aliases(
        DATA / "PropertyValueAliases.txt"
    )
    binary_alias = parse_binary_aliases(
        DATA / "PropertyAliases.txt", set(ES_BINARY)
    )

    binaries = defaultdict(list)
    for f in ("PropList.txt", "DerivedCoreProperties.txt",
              "DerivedNormalizationProps.txt", "emoji/emoji-data.txt",
              "extracted/DerivedBinaryProperties.txt"):
        for value, ranges in parse_ranges(DATA / f).items():
            if value in ES_BINARY:
                binaries[value].extend(ranges)

    gc = {v: merge(r) for v, r in gc.items()}
    # Cn (unassigned) is the complement of everything listed; the
    # extracted file DOES list Cn explicitly — trust it if present.
    if "Cn" not in gc:
        assigned = merge([r for rs in gc.values() for r in rs])
        gc["Cn"] = complement(assigned)
    for comp, leaves in GC_COMPOSITES.items():
        gc[comp] = merge([r for leaf in leaves for r in gc[leaf]])

    scripts = {v: merge(r) for v, r in scripts.items()}
    # Script_Extensions: ScriptExtensions.txt only lists cps whose scx
    # differs from their sc; everywhere else scx == sc. Per UTS24 the
    # scx set for other cps is {sc(cp)} — so scx(X) = sc(X) minus the
    # cps reassigned away, plus the cps listing X. Practically:
    # scx(X) = (sc(X) - domain(ScriptExtensions.txt)) + listed(X).
    listed_domain = merge([r for rs in scx_by_short.values() for r in rs])

    def subtract(ranges, cut):
        out = []
        for lo, hi in ranges:
            cur = lo
            for clo, chi in cut:
                if chi < cur or clo > hi:
                    continue
                if clo > cur:
                    out.append((cur, clo - 1))
                cur = max(cur, chi + 1)
                if cur > hi:
                    break
            if cur <= hi:
                out.append((cur, hi))
        return out

    scx = {}
    for long_name, sc_ranges in scripts.items():
        scx[long_name] = subtract(sc_ranges, listed_domain)
    for short, ranges in scx_by_short.items():
        long_name = sc_short_to_long.get(short, short)
        scx.setdefault(long_name, [])
        scx[long_name] = merge(scx[long_name] + ranges)
    scx = {v: merge(r) for v, r in scx.items()}

    binaries = {v: merge(r) for v, r in binaries.items()}
    binaries["Any"] = [(0, MAX_CP)]
    all_assigned = merge([r for v, rs in gc.items()
                          if v != "Cn" and v not in GC_COMPOSITES
                          for r in rs])
    binaries["Assigned"] = all_assigned
    binaries["ASCII"] = [(0, 0x7F)]
    missing = [b for b in ES_BINARY if b not in binaries]
    if missing:
        print(f"MISSING binary properties: {missing}", file=sys.stderr)
        sys.exit(1)

    out = []
    out.append("// CODEGEN: labs/ucd-gen/gen.py from UCD 16.0.0 — do not edit.")
    out.append("//! Unicode property range tables for `\\p{...}` escapes")
    out.append("//! (RFC 20260711-regex-property-escapes chunk A).")
    out.append("")
    out.append("use crate::ucd::UPropRange;")
    out.append("")

    def emit_family(prefix, table, canonical_sort):
        names = sorted(table.keys())
        for name in names:
            out.append(rs_ranges(f"{prefix}_{sanitize(name)}", table[name]))
        entries = "".join(
            f'    ("{n}", {prefix}_{sanitize(n)}),\n' for n in sorted(names, key=canonical_sort)
        )
        out.append(
            f"pub static {prefix}_TABLES: &[(&str, &[UPropRange])] = &[\n{entries}];\n"
        )

    emit_family("GC", gc, str)
    emit_family("SCRIPT", scripts, str)
    emit_family("SCX", scx, str)
    emit_family("BINARY", binaries, str)

    def emit_alias(name, table):
        entries = "".join(
            f'    ("{a}", "{c}"),\n' for a, c in sorted(table.items())
        )
        out.append(f"pub static {name}: &[(&str, &str)] = &[\n{entries}];\n")

    emit_alias("GC_ALIASES", gc_alias)
    emit_alias("SCRIPT_ALIASES", sc_alias)
    emit_alias("BINARY_ALIASES", binary_alias)

    total = sum(len(r) for t in (gc, scripts, scx, binaries) for r in t.values())
    print(f"// {total} ranges across {len(gc)}+{len(scripts)}+{len(scx)}+{len(binaries)} tables", file=sys.stderr)
    print("\n".join(out))


if __name__ == "__main__":
    main()
