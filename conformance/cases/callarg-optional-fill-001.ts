// chunk 784 — absent optional fields fill at direct-call argument
// sites (`f({ n: 5 })` against `(o: O)`), mirroring the chunk-780
// let-decl fill; previously a loud checker reject.
type O = { tag?: string, n: number };
function f(o: O): string {
  if (o.tag) { return String(o.n) + o.tag }
  return String(o.n) + "-"
}
console.log(f({ n: 5 }));
console.log(f({ tag: "x", n: 6 }));
