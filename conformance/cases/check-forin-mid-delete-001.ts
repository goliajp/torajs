// chunk B3 (RFC 20260711 for-in) — ES §14.7.5: properties deleted
// during enumeration must not be visited. The lowering re-checks each
// snapshot key against the live object (any_prop_has) before the body.
const o: any = { a: 1, b: 2, c: 3 };
for (const k in o) {
  console.log(k);
  delete o.c;
}
console.log("---");
const p: any = { x: 1, y: 2, z: 3 };
for (const k in p) {
  console.log(k);
  if (k === "x") {
    delete p.y;
  }
}
console.log("---");
const q: any = { m: 1, n: 2 };
for (const k in q) {
  console.log(k, q[k]);
  delete q.n;
  q.w = 9;
}
console.log("done");
