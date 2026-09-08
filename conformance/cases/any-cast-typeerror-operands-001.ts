// `x as any` leaves a heap value's SSA type alone — only primitives
// and the nullish pointer box there — so the checker says Any while
// the lowering still sees the concrete type. For every type that HAS
// a concrete arm that is harmless; for the two whose only correct
// answer is a TypeError it was not. No arm matched, the pair fell
// through to the integer lane, and the program printed the POINTER:
// `"x" + (Symbol("a") as any)` gave `8689029528`, `(1n as any) * 2`
// gave `8676311200`. The identical programs through a `const x: any`
// binding — whose slot really is boxed — threw correctly all along.
const sym = Symbol("a");

function caught(f: () => unknown): string {
  try { f(); return "no throw"; } catch (e: any) { return e.constructor.name; }
}

console.log(caught(() => "x" + (sym as any)));
console.log(caught(() => (sym as any) + "x"));
console.log(caught(() => "x" + (Symbol("b") as any)));
console.log(caught(() => `${sym as any}`));
console.log(caught(() => (sym as any) * 2));
console.log(caught(() => (sym as any) < 2));
console.log(caught(() => (sym as any) | 0));

console.log(caught(() => (1n as any) * 2));
console.log(caught(() => (1n as any) - 2));
console.log(caught(() => 2 - (1n as any)));
console.log(caught(() => (1n as any) + 2));
console.log(caught(() => (1n as any) & 1));

// What stays out is as load-bearing as what goes in.
// A Symbol still compares by identity, prints, and keys an object.
console.log(sym === sym, sym === Symbol("a"), typeof sym, String(sym), sym.description);
const bag: any = {};
bag[sym] = 7;
console.log(bag[sym]);

// §7.2.13 IS defined across the numeric types, so ordering keeps its
// concrete arm, and a String partner keeps §13.15.3's concat branch.
console.log((1n as any) < 2, 1n < 2, "x" + 1n, 1n + "x");
console.log(1n * 2n, 1n + 2n, 5n / 2n, 2n ** 3n, 1n & 3n);
