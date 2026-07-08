// Chunk 699 — re.test(s) / re.exec(s) with a Substr-view haystack
// (a for-of-str binding, a slice-produced view). The regex byte
// reader misread the 16-byte parent-pointer block as an owned Str
// (probe: /a/.test(ch) answered false on a matching char; the
// 2026-07-03 audit recorded a UTF-16 SIGBUS risk on the same
// shape). The haystack now materializes through substr_to_owned at
// the lowering (a fresh temp dropped after the call).
for (const ch of "aXa") {
  console.log(/a/.test(ch));
}
const s = "hello abbba world";
const view = s.slice(6, 11);
console.log(/ab+a/.test(view));
const m = /b+/.exec(view);
console.log(m);
// exec answers null on a miss through the same lane
for (const c of "zq") {
  console.log(/a/.exec(c));
}
// owned-Str haystack regression (pass-through, no materialize)
console.log(/world/.test(s));
