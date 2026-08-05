// `T | undefined` — the longhand of the optional shape `T?`, which
// §9.2 says are the same type. Both now parse, and both bind a real
// undefined rather than a null standing in for one.
function scope(): void {
  const a: string | undefined = undefined;
  console.log(a, a === undefined, a === null, typeof a);
  const b: number | undefined = 5;
  console.log(b, b === undefined, typeof b);
  const c: string | null = null;
  console.log(c, c === null, typeof c);
}
scope();

// explicit-union parameters, passed both ways
function f(x: string | undefined): string {
  return x === undefined ? "none" : x;
}
console.log(f(undefined), f("hi"));

function k(s: string | null): string {
  return typeof s + "/" + String(s === null);
}
console.log(k(null), k("a"));

// an omitted optional binds undefined at every width — not the
// type's zero, and not null
function g(x?: string): string {
  return x === undefined ? "none" : x;
}
function h(n?: number): string {
  return n === undefined ? "no-n" : String(n);
}
function bo(f2?: boolean): string {
  return f2 === undefined ? "no-f" : String(f2);
}
function an(v?: any): string {
  return v === undefined ? "no-v" : String(v);
}
console.log(g(), g("hi"));
console.log(h(), h(3));
console.log(bo(), bo(true));
console.log(an(), an(1));

// typeof / === agree with the value at every optional width
function probe(n?: number, s?: string, f3?: boolean): void {
  console.log(typeof n, n === undefined, n === null);
  console.log(typeof s, s === undefined, s === null);
  console.log(typeof f3, f3 === undefined, f3 === null);
}
probe();
probe(1, "a", true);
