// chunk B2 (RFC 20260711 for-in) — var / bare for-in and for-of head
// forms + ES §14.7.5 null/undefined zero-iteration + primitive
// receivers through the __forinKeys desugar.
const o: any = { a: 1, b: 2, c: 3 };
for (var k in o) {
  console.log(k);
}
console.log("var leaks", k);
let bare = "";
for (bare in o) {
  if (bare === "b") {
    continue;
  }
  console.log("bare", bare);
}
console.log("after", bare);
for (var v of ["x", "y"]) {
  console.log(v);
}
console.log("of leaks", v);
let e = 0;
for (e of [7, 8]) {
}
console.log("bare of", e);
const nul: any = null;
for (const n in nul) {
  console.log("never");
}
const und: any = undefined;
for (const u in und) {
  console.log("never");
}
const num: any = 42;
for (const q in num) {
  console.log("never");
}
const s: any = "ab";
for (const i in s) {
  console.log("str key", i);
}
for (var empty in {}) {
}
console.log("empty leak", empty);
console.log("done");
