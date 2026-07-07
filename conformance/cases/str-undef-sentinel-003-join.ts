// RFC 20260707-undefined-sentinel-repr chunk 4 — a nullish Str slot
// (undefined sentinel from a missed capture or an array-literal
// undefined) joins as the empty string per ES §23.1.3.18 step 8.c,
// never its payload text.

const m = /a(b)?/.exec("a");
if (m !== null) {
  console.log(m.join(","));
  console.log(m.join("|"));
  console.log(m.join(""));
}
const e = ["a", undefined, "c"];
console.log(e.join(","));
const h = /a(b)/.exec("ab");
if (h !== null) {
  console.log(h.join(","));
}
console.log("done");
