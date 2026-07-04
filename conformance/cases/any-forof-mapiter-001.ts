// RFC 20260704 S5+ — for-of over `any`-held Map/Set iterator cells
// (keys()/values()/entries() minted by the C4+ any-method-call
// surface) via the unified runtime iteration protocol
// (__torajs_any_iter_next tag-dispatch; one call per step).
const m: any = new Map();
m.set("a", 1);
m.set("b", 2);
for (const k of m.keys()) {
  console.log(k);
}
for (const v of m.values()) {
  console.log(v);
}
for (const e of m.entries()) {
  console.log(e[0]);
  console.log(e[1]);
}
const s: any = new Set();
s.add(10);
s.add(20);
for (const x of s.keys()) {
  console.log(x);
}
for (const p of s.entries()) {
  console.log(p[1]);
}
// half-consumed iterator resumes where next() left off
const ki: any = m.keys();
ki.next();
for (const rest of ki) {
  console.log(rest);
}
// exhausted iterator: body must not run
for (const z of ki) {
  console.log("SHOULD NOT PRINT");
}
console.log("done");
