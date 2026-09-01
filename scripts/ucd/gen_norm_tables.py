#!/usr/bin/env python3
"""Generate torajs-str's normalization tables from UCD UnicodeData.txt +
DerivedNormalizationProps.txt.

Usage:
    python3 gen_norm_tables.py <ucd-dir> > crates/torajs-str/src/norm_table.rs

`<ucd-dir>` is a directory containing UnicodeData.txt +
DerivedNormalizationProps.txt (download from
https://www.unicode.org/Public/<ver>/ucd/). The UCD version itself is
captured in a header comment so we know which release the tables track.

Same host-side codegen pattern as gen_case_tables.py: the output
`norm_table.rs` is checked in. UCD upgrade = re-run the script with new
data and commit the diff. We do NOT pull UCD at build time (0 build-time
deps; the generated tables are plain Rust source).

Output tables (per P11.6 RFC §3.1):

- `DECOMP_POOL: &[u32]` — flat pool of all decomposition target code
  points, indexed by `pool_off`.
- `DECOMP_TABLE: &[DecompEntry]` — sorted by source cp.
  `DecompEntry { source: u32, pool_off: u32, len: u8, is_compat: u8 }`.
  Hangul syllables (U+AC00..D7A3) are NOT in this table — they use the
  algorithmic decomposition formula (UAX #15 D118).
- `CCC_TABLE: &[(u32, u8)]` — sparse, sorted by cp. cps not present
  have CCC = 0.
- `COMPOSE_TABLE: &[((u32, u32), u32)]` — sorted by (starter,
  combining). Built by inverting binary canonical decompositions,
  excluding Full_Composition_Exclusion entries (UAX #15 §3 D118
  primary-composite gating).
- `NFC_QC_BITMAP` / `NFD_QC_BITMAP` / `NFKC_QC_BITMAP` /
  `NFKD_QC_BITMAP: &[u8]` — 2-bit packed quick-check tables. cps
  beyond the bitmap length default to Y (already-normalized). Encoding:
  0 = Y, 1 = N, 2 = M. 4 cps per byte (cp 0 in low 2 bits).

Caller is responsible for:
- Decoding the input string to code points.
- Per-cp NFC/NFD recursion (handle Hangul algorithmically, then table
  lookup, then identity).
- Canonical reordering (CCC bubble sort, stable, contiguous CCC>0 run).
- Compose pass (canonical primary composite + Hangul L+V / LV+T
  algorithmic).
- Quick-check short-circuit before running full algorithm.
- Encoding output via canonical encoding (Latin-1 vs UTF-16 inferred).

DOES NOT cover: `isWellFormed` / `toWellFormed` (orthogonal surface;
separate trunk item).
"""

import sys
import os

def ucd_version_of(dnp_path):
    """The DerivedNormalizationProps.txt header names its UCD version
    (`# DerivedNormalizationProps-17.0.0.txt`)."""
    with open(dnp_path) as f:
        first = f.readline().strip()
    return first.split("DerivedNormalizationProps-")[1].split(".txt")[0]


UCD_VERSION = "unknown"

# Hangul algorithmic constants per UAX #15 D118-D119.
SBASE = 0xAC00
SCOUNT = 11172
HANGUL_END = SBASE + SCOUNT  # exclusive


def parse_unicode_data(path):
    """Return (decomp, ccc):
      decomp: cp -> (is_compat: bool, [cp, ...])
      ccc:    cp -> int (only entries != 0)
    """
    decomp = {}
    ccc = {}
    with open(path) as f:
        for line in f:
            line = line.rstrip("\n")
            if not line or line.startswith("#"):
                continue
            fields = line.split(";")
            if len(fields) < 15:
                continue
            cp = int(fields[0], 16)
            ccc_val = int(fields[3])
            if ccc_val != 0:
                ccc[cp] = ccc_val
            dm = fields[5].strip()
            if not dm:
                continue
            is_compat = False
            if dm.startswith("<"):
                is_compat = True
                # strip "<tag> "
                rb = dm.index(">")
                dm = dm[rb + 1:].strip()
            targets = [int(t, 16) for t in dm.split() if t]
            if SBASE <= cp < HANGUL_END:
                # Hangul precomposed — algorithmic; don't emit table entry
                continue
            decomp[cp] = (is_compat, targets)
    return decomp, ccc


def parse_qc_and_exclusion(path):
    """Return (qc_map, full_exclusion).
      qc_map: prop_name -> {cp: 'N'|'M'}, only N/M entries (Y is default).
        prop_name in {NFC_QC, NFD_QC, NFKC_QC, NFKD_QC}
      full_exclusion: set[cp] — Full_Composition_Exclusion.
    """
    QC_PROPS = {"NFC_QC", "NFD_QC", "NFKC_QC", "NFKD_QC"}
    qc_map = {p: {} for p in QC_PROPS}
    full_exclusion = set()
    with open(path) as f:
        for raw in f:
            raw = raw.rstrip("\n")
            if not raw or raw.startswith("#"):
                continue
            comment_idx = raw.find("#")
            payload = raw[:comment_idx] if comment_idx >= 0 else raw
            fields = [s.strip() for s in payload.split(";")]
            if len(fields) < 2:
                continue
            cp_range = fields[0]
            prop = fields[1]
            if ".." in cp_range:
                lo_s, hi_s = cp_range.split("..")
                lo, hi = int(lo_s, 16), int(hi_s, 16)
            else:
                lo = hi = int(cp_range, 16)
            if prop == "Full_Composition_Exclusion":
                for cp in range(lo, hi + 1):
                    full_exclusion.add(cp)
            elif prop in QC_PROPS:
                if len(fields) < 3:
                    continue
                val = fields[2]
                if val in ("N", "M"):
                    for cp in range(lo, hi + 1):
                        qc_map[prop][cp] = val
    return qc_map, full_exclusion


def build_decomp_pool_and_table(decomp):
    """Return (pool, entries):
      pool: list[int] — flat u32 pool of target code points.
      entries: list[(source_cp, pool_off, len, is_compat_int)] sorted by cp.
    """
    pool = []
    entries = []
    for cp in sorted(decomp):
        is_compat, targets = decomp[cp]
        off = len(pool)
        pool.extend(targets)
        entries.append((cp, off, len(targets), 1 if is_compat else 0))
    return pool, entries


def build_compose_table(decomp, full_exclusion):
    """Return list[((a, b), composite)] sorted by (a, b).
    Built by inverting binary canonical decompositions, filtering out
    Full_Composition_Exclusion sources.
    """
    pairs = []
    for cp, (is_compat, targets) in decomp.items():
        if is_compat:
            continue
        if len(targets) != 2:
            continue
        if cp in full_exclusion:
            continue
        a, b = targets[0], targets[1]
        pairs.append(((a, b), cp))
    pairs.sort(key=lambda kv: kv[0])
    return pairs


def build_qc_bitmaps(qc_map):
    """Return (max_cp_inclusive, dict[prop_name -> bytes]).
    Each bitmap is 2-bit packed: 0=Y, 1=N, 2=M. 4 cps per byte
    (cp%4 == 0 in low bits).
    Length covers [0, max_cp_inclusive], rounded up to byte boundary.
    cps beyond bitmap default to Y.
    """
    max_cp = 0
    for prop in qc_map:
        if qc_map[prop]:
            max_cp = max(max_cp, max(qc_map[prop]))
    bytes_len = (max_cp // 4) + 1
    bitmaps = {}
    for prop, mp in qc_map.items():
        buf = bytearray(bytes_len)
        for cp, val in mp.items():
            v = 1 if val == "N" else 2  # M
            byte_idx = cp // 4
            bit_off = (cp % 4) * 2
            buf[byte_idx] |= (v << bit_off)
        bitmaps[prop] = bytes(buf)
    return max_cp, bitmaps


# ---------------- emit ----------------

def emit_header():
    print("// CODEGEN: scripts/ucd/gen_norm_tables.py — file-size audit carve-out (file-size.md #1).")
    print("//! UCD normalization tables — auto-generated by")
    print("//! `scripts/ucd/gen_norm_tables.py` from UnicodeData.txt +")
    print("//! DerivedNormalizationProps.txt. DO NOT EDIT BY HAND.")
    print("//!")
    print(f"//! UCD version: {UCD_VERSION}")
    print("//!")
    print("//! Tables (per P11.6 RFC §3.1):")
    print("//! - `DECOMP_POOL` / `DECOMP_TABLE`: cp -> [cp, ...] decomposition;")
    print("//!   `is_compat` bit distinguishes canonical-only from compat-only entries.")
    print("//!   Hangul syllables (U+AC00..D7A3) excluded — handled algorithmically.")
    print("//! - `CCC_TABLE`: sparse cp -> CCC, sorted by cp; absent = CCC 0.")
    print("//! - `COMPOSE_TABLE`: sorted ((starter, combining)) -> composite;")
    print("//!   Full_Composition_Exclusion entries filtered out at generation time.")
    print("//! - `NFC_QC_BITMAP` / `NFD_QC_BITMAP` / `NFKC_QC_BITMAP` /")
    print("//!   `NFKD_QC_BITMAP`: 2-bit packed (0=Y, 1=N, 2=M), 4 cps per byte.")
    print("//!")
    print("//! All slice tables sorted by source for binary search.")
    print()
    print("#![allow(clippy::unreadable_literal)]")
    print()
    print('#[allow(dead_code)]')
    print(f'pub(crate) const UCD_VERSION: &str = "{UCD_VERSION}";')
    print()


def emit_decomp_entry_struct():
    print("/// One UnicodeData column-5 decomposition mapping.")
    print("/// `is_compat = 1` -> entry came from a `<tag>` (NFKD/NFKC consumers).")
    print("#[allow(dead_code)]")
    print("#[derive(Clone, Copy)]")
    print("pub(crate) struct DecompEntry {")
    print("    pub source: u32,")
    print("    pub pool_off: u32,")
    print("    pub len: u8,")
    print("    pub is_compat: u8,")
    print("}")
    print()


def emit_pool(name, pool):
    print(f"/// {len(pool)} u32 code points; flat pool indexed by `DECOMP_TABLE` entries.")
    print(f"#[allow(dead_code)]")
    print(f"pub(crate) static {name}: &[u32] = &[")
    LINE = 8
    for i in range(0, len(pool), LINE):
        chunk = pool[i:i+LINE]
        print("    " + ", ".join(f"0x{cp:04X}" for cp in chunk) + ",")
    print("];")
    print()


def emit_decomp_table(name, entries):
    print(f"/// {len(entries)} entries, sorted by source cp.")
    print(f"#[allow(dead_code)]")
    print(f"pub(crate) static {name}: &[DecompEntry] = &[")
    for (source, off, length, is_compat) in entries:
        print(f"    DecompEntry {{ source: 0x{source:04X}, pool_off: {off}, len: {length}, is_compat: {is_compat} }},")
    print("];")
    print()


def emit_ccc_table(name, ccc):
    items = sorted(ccc.items())
    print(f"/// {len(items)} cps with non-zero CCC, sorted.")
    print(f"#[allow(dead_code)]")
    print(f"pub(crate) static {name}: &[(u32, u8)] = &[")
    LINE = 6
    for i in range(0, len(items), LINE):
        chunk = items[i:i+LINE]
        line = ", ".join(f"(0x{cp:04X}, {v})" for (cp, v) in chunk)
        print(f"    {line},")
    print("];")
    print()


def emit_compose_table(name, compose):
    print(f"/// {len(compose)} primary-composite pairs after Full_Composition_Exclusion filter.")
    print(f"/// Sorted by (starter, combining).")
    print(f"#[allow(dead_code)]")
    print(f"pub(crate) static {name}: &[((u32, u32), u32)] = &[")
    for ((a, b), c) in compose:
        print(f"    ((0x{a:04X}, 0x{b:04X}), 0x{c:04X}),")
    print("];")
    print()


def emit_qc_bitmap(name, blob):
    print(f"/// 2-bit packed quick-check bitmap (0=Y, 1=N, 2=M). 4 cps per byte. {len(blob)} bytes.")
    print(f"#[allow(dead_code)]")
    print(f"pub(crate) static {name}: &[u8] = &[")
    LINE = 16
    for i in range(0, len(blob), LINE):
        chunk = blob[i:i+LINE]
        line = ", ".join(f"0x{b:02X}" for b in chunk)
        print(f"    {line},")
    print("];")
    print()


def emit_lookup_helpers(qc_max_cp):
    print("// ---------------- lookup helpers ----------------")
    print()
    print("#[allow(dead_code)]")
    print("pub(crate) fn lookup_decomp(cp: u32) -> Option<(&'static [u32], bool)> {")
    print("    let idx = DECOMP_TABLE.binary_search_by_key(&cp, |e| e.source).ok()?;")
    print("    let e = DECOMP_TABLE[idx];")
    print("    let off = e.pool_off as usize;")
    print("    let len = e.len as usize;")
    print("    Some((&DECOMP_POOL[off..off + len], e.is_compat != 0))")
    print("}")
    print()
    print("#[allow(dead_code)]")
    print("pub(crate) fn lookup_ccc(cp: u32) -> u8 {")
    print("    match CCC_TABLE.binary_search_by_key(&cp, |&(k, _)| k) {")
    print("        Ok(i) => CCC_TABLE[i].1,")
    print("        Err(_) => 0,")
    print("    }")
    print("}")
    print()
    print("#[allow(dead_code)]")
    print("pub(crate) fn lookup_composite(a: u32, b: u32) -> Option<u32> {")
    print("    let idx = COMPOSE_TABLE.binary_search_by_key(&(a, b), |&(k, _)| k).ok()?;")
    print("    Some(COMPOSE_TABLE[idx].1)")
    print("}")
    print()
    print("/// Returns 0 (Yes), 1 (No), or 2 (Maybe). cps beyond bitmap default to Yes.")
    print("#[allow(dead_code)]")
    print("#[inline]")
    print("pub(crate) fn lookup_qc(bitmap: &[u8], cp: u32) -> u8 {")
    print("    let byte_idx = (cp / 4) as usize;")
    print("    if byte_idx >= bitmap.len() {")
    print("        return 0;")
    print("    }")
    print("    let bit_off = (cp % 4) * 2;")
    print("    (bitmap[byte_idx] >> bit_off) & 0b11")
    print("}")
    print()
    print(f"/// Largest cp with a non-Y QC entry across all 4 bitmaps. cps > {qc_max_cp:#X} short-circuit to Y.")
    print(f"#[allow(dead_code)]")
    print(f"pub(crate) const QC_MAX_CP: u32 = 0x{qc_max_cp:X};")
    print()


def main(argv):
    if len(argv) != 2:
        sys.stderr.write("usage: gen_norm_tables.py <ucd-dir>\n")
        sys.exit(2)
    ucd = argv[1]
    ud_path = os.path.join(ucd, "UnicodeData.txt")
    dnp_path = os.path.join(ucd, "DerivedNormalizationProps.txt")
    global UCD_VERSION
    UCD_VERSION = ucd_version_of(dnp_path)
    for p in (ud_path, dnp_path):
        if not os.path.exists(p):
            sys.stderr.write(f"missing UCD file: {p}\n")
            sys.exit(2)

    decomp, ccc = parse_unicode_data(ud_path)
    qc_map, full_exclusion = parse_qc_and_exclusion(dnp_path)
    pool, entries = build_decomp_pool_and_table(decomp)
    compose = build_compose_table(decomp, full_exclusion)
    qc_max_cp, bitmaps = build_qc_bitmaps(qc_map)

    sys.stderr.write(
        f"stats: decomp_entries={len(entries)} pool_cps={len(pool)} "
        f"ccc={len(ccc)} compose={len(compose)} qc_max_cp=U+{qc_max_cp:04X} "
        f"bitmap_bytes={len(bitmaps['NFC_QC'])}\n"
    )

    emit_header()
    emit_decomp_entry_struct()
    emit_pool("DECOMP_POOL", pool)
    emit_decomp_table("DECOMP_TABLE", entries)
    emit_ccc_table("CCC_TABLE", ccc)
    emit_compose_table("COMPOSE_TABLE", compose)
    emit_qc_bitmap("NFC_QC_BITMAP", bitmaps["NFC_QC"])
    emit_qc_bitmap("NFD_QC_BITMAP", bitmaps["NFD_QC"])
    emit_qc_bitmap("NFKC_QC_BITMAP", bitmaps["NFKC_QC"])
    emit_qc_bitmap("NFKD_QC_BITMAP", bitmaps["NFKD_QC"])
    emit_lookup_helpers(qc_max_cp)


if __name__ == "__main__":
    main(sys.argv)
