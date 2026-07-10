// chunk 788 — the optional-field fill recurses into struct-typed
// fields whose value is itself an ObjectLit: `{ o: {} }` fills the
// inner literal against O's declared list at let-decl AND call-arg
// sites (chunk 784 only jobbed the outer literal, so a short inner
// literal stayed a loud checker reject).
type O = { tag?: string, n: number };
type H = { o: O };
const h: H = { o: { n: 1 } };
console.log(h.o.tag ?? "-");
console.log(h.o.n);
function f(x: H): string { return x.o.tag ?? "fn-" }
console.log(f({ o: { n: 2 } }));
