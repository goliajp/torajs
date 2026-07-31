// §12.9.3 NumericLiteralSeparator + BigIntLiteralSuffix strictness
// (rotation 264 half-blade A — every shape here is LEGAL; the
// illegal twins `1_` / `1__2` / `0_0` / `1_.5` / `1e2_` / `0x_1` /
// `0b1_` / `1_e2` / `01n` / `08n` are lex rejects, pinned by the
// runner's compile-reject path and the probe matrix in the commit).
const a = 1_000_000;
console.log(a);
const b = 0.5_5;
console.log(b);
const c = 1e1_0;
console.log(c);
const d = 0xf_f;
console.log(d);
const e = 0b1_01;
console.log(e);
const f = 0o7_7;
console.log(f);
const g = 3.1_4;
console.log(g);
const h = 0n;
console.log(h);
const i = 123_456n;
console.log(i);
const j = 0x1_fn;
console.log(j);
