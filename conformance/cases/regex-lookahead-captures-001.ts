// Chunk 639 — captures inside a POSITIVE lookahead body survive the
// assertion (ES §22.2.2: slots are global across the pattern; a
// successful lookaround keeps its body's SAVEs). Pre-fix the
// sub-probe answered bool-only and the parent's no-saves fast path
// (has_save scanned parent insts only) short-circuited the whole
// saves channel — S15.10.2.8_A1_T1's `/(?=(a+))/` answered null.
const m = /(?=(a+))/.exec("baaabac");
console.log(m === null ? "null" : "hit");
if (m !== null) {
  console.log(m.length);
  console.log(m.index);
  console.log(m[0] === "" ? "<empty>" : m[0]);
  console.log(m[1]);
}
// lookahead capture + consuming continuation
const m2 = /(?=(ab))a/.exec("xab");
console.log(m2 === null ? "null2" : m2[0] + "|" + m2[1]);
// negative lookahead never contributes captures
const m3 = /(?!(x))b/.exec("ab");
console.log(m3 === null ? "null3" : m3[0] + "|" + (m3[1] === undefined ? "undef" : m3[1]));
// lookahead group alongside a consuming group — both populated
const m4 = /(?=(a+))(a)/.exec("baaab");
console.log(m4 === null ? "null4" : m4[0] + "|" + m4[1] + "|" + m4[2]);
// global replace driven through a lookahead capture
console.log("a1 a2".replace(/(?=(\d))a/g, "n"));
// test() path over the same shape
console.log(/(?=(b+))b/.test("abb"));
console.log("done");
