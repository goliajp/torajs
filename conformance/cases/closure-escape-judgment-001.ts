// RFC 20260824-s2-5 刀 4 A1 (link-judged form) — a closure that is
// invoked through the any world keeps its `__boxed_` adapter: every
// dynamic call site below reads the cell's boxed entry through the
// one rc entry the link guards on, so the mints stay. The crossings:
// an any-typed object field, an any-typed parameter, `.call`, and an
// any-lane array element. A closure only ever called directly (the
// `plain` one) is no evidence either way.
const dbl = (x: number): number => x * 2;
const bag: any = { f: dbl };
console.log(bag.f(21));
function viaParam(g: any): number {
  return g(4);
}
console.log(viaParam((y: number) => y + 1));
const asAny: any = (s: string): string => s + "!";
console.log(asAny.call(null, "hi"));
const fns: any[] = [(n: number) => n - 1];
console.log(fns[0](10));
const plain = (z: number): number => z * z;
console.log(plain(7));
