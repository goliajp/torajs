// 424-01 — the `name?: T` optional-parameter marker inside a
// fn-type annotation. Refusing the spelling made every program
// carrying a TS-idiomatic callback annotation a parse error; the
// marker encodes as the SAME `__nullable(T)` a value-side
// `(b?: string) =>` parameter carries, and an omitted trailing
// optional binds undefined through the T-28 pad (§10.2.1.4).
function g(p = 42) {
  console.log("g:", p);
}
function callit(f: (p?: number) => void) {
  f();
  f(7);
}
callit(g);
const h: (a: number, b?: string) => string = (a: number, b?: string) => a + (b ?? "!");
console.log(h(1, "x"), h(2, undefined));
function tail(f: (a: number, b?: number, c?: boolean) => void) {
  f(1);
  f(1, 2);
  f(1, 2, true);
}
tail((a: number, b?: number, c?: boolean) => {
  console.log("tail:", a, b, c);
});
