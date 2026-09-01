// Unicode 17.0 added case pairs in Latin Extended-D: U+A7CB/A7CC/A7CD
// and U+A7CE/A7CF, U+A7D2/A7D3, U+A7D4/A7D5. The default case
// conversion tables (§22.1.3.29 / .30) must carry them.
const pairs: [number, number][] = [
  [0xa7ce, 0xa7cf],
  [0xa7d2, 0xa7d3],
  [0xa7d4, 0xa7d5],
  [0xa7cb, 0x0264],
  [0xa7cc, 0xa7cd],
];
for (const [up, low] of pairs) {
  const u = String.fromCharCode(up);
  const l = String.fromCharCode(low);
  console.log(
    up.toString(16),
    u.toLowerCase().charCodeAt(0).toString(16),
    l.toUpperCase().charCodeAt(0).toString(16),
    u.toLowerCase() === l,
    l.toUpperCase() === u,
  );
}
// unchanged neighbours stay identity in both directions
for (const cp of [0xa7d0, 0xa7d1, 0xa7d6]) {
  const s = String.fromCharCode(cp);
  console.log(cp.toString(16), s.toUpperCase().charCodeAt(0).toString(16), s.toLowerCase().charCodeAt(0).toString(16));
}
