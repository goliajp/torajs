#!/usr/bin/env python3
"""UCD 17.0 emoji sequences -> crates/torajs-regex/src/ucd_emoji_seq.rs.

RFC 20260712-regex-vflag-modifiers chunk B3. Reads
data/emoji/emoji-sequences.txt + emoji-zwj-sequences.txt (downloaded
from unicode.org/Public/17.0.0/emoji/) and emits one CODEGEN Rust
file with the ES section 22.2.1 "properties of strings" tables:

- Basic_Emoji, Emoji_Keycap_Sequence, RGI_Emoji_Flag_Sequence,
  RGI_Emoji_Modifier_Sequence, RGI_Emoji_Tag_Sequence (sequences file)
- RGI_Emoji_ZWJ_Sequence (zwj file)
- RGI_Emoji = union of all six (materialised as part references, not
  duplicated data — the parser unions parts at fold time)

Each property = sorted UPropRange list (single-cp entries / ranges)
+ sorted &str list (multi-cp sequences, UTF-8).

Usage: python3 gen_emoji.py > ../../crates/torajs-regex/src/ucd_emoji_seq.rs
"""

from collections import defaultdict
from pathlib import Path

DATA = Path(__file__).parent / "data" / "emoji"

PROPS = [
    "Basic_Emoji",
    "Emoji_Keycap_Sequence",
    "RGI_Emoji_Flag_Sequence",
    "RGI_Emoji_Modifier_Sequence",
    "RGI_Emoji_Tag_Sequence",
    "RGI_Emoji_ZWJ_Sequence",
]


def parse_file(path):
    out = defaultdict(lambda: ([], []))  # prop -> (ranges, seqs)
    for line in open(path, encoding="utf-8"):
        line = line.split("#", 1)[0].strip()
        if not line:
            continue
        fields = [f.strip() for f in line.split(";")]
        cps, prop = fields[0], fields[1]
        ranges, seqs = out[prop]
        if ".." in cps:
            lo, hi = cps.split("..")
            ranges.append((int(lo, 16), int(hi, 16)))
        else:
            parts = [int(c, 16) for c in cps.split()]
            if len(parts) == 1:
                ranges.append((parts[0], parts[0]))
            else:
                seqs.append(parts)
    return out


def merge(ranges):
    out = []
    for lo, hi in sorted(ranges):
        if out and lo <= out[-1][1] + 1:
            out[-1] = (out[-1][0], max(out[-1][1], hi))
        else:
            out.append((lo, hi))
    return out


def rust_str(cps):
    return '"' + "".join(f"\\u{{{cp:X}}}" for cp in cps) + '"'


def main():
    table = defaultdict(lambda: ([], []))
    for f in ["emoji-sequences.txt", "emoji-zwj-sequences.txt"]:
        for prop, (ranges, seqs) in parse_file(DATA / f).items():
            table[prop][0].extend(ranges)
            table[prop][1].extend(seqs)
    missing = [p for p in PROPS if p not in table]
    assert not missing, f"missing properties: {missing}"

    out = []
    out.append("// CODEGEN: labs/ucd-gen/gen_emoji.py from UCD 17.0 emoji data — do not edit.")
    out.append("//! ES §22.2.1 \"properties of strings\" tables for v-flag")
    out.append("//! `\\p{RGI_Emoji}`-family escapes (RFC 20260712 chunk B3).")
    out.append("")
    out.append("use crate::ucd::{StringProp, UPropRange};")
    out.append("")
    total_r = total_s = 0
    for prop in PROPS:
        ranges, seqs = table[prop]
        ranges = merge(ranges)
        seqs = sorted(seqs)
        total_r += len(ranges)
        total_s += len(seqs)
        up = prop.upper()
        out.append(f"static {up}_CP: [UPropRange; {len(ranges)}] = [")
        for lo, hi in ranges:
            out.append(f"    UPropRange {{ lo: 0x{lo:X}, hi: 0x{hi:X} }},")
        out.append("];")
        out.append(f"static {up}_SEQ: [&str; {len(seqs)}] = [")
        for s in seqs:
            out.append(f"    {rust_str(s)},")
        out.append("];")
        out.append(f"pub static {up}: StringProp = StringProp {{")
        out.append(f"    cp_ranges: &{up}_CP,")
        out.append(f"    strings: &{up}_SEQ,")
        out.append("};")
        out.append("")
    parts = ", ".join(f"&{p.upper()}" for p in PROPS)
    out.append("/// `RGI_Emoji` = union of the six component properties (UTS #51).")
    out.append(f"pub static RGI_EMOJI_PARTS: [&StringProp; {len(PROPS)}] = [{parts}];")
    print("\n".join(out))
    import sys

    print(f"// {total_r} ranges / {total_s} sequences", file=sys.stderr)


if __name__ == "__main__":
    main()
