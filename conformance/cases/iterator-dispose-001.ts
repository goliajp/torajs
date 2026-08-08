// RFC 20260809 B6 — %Iterator.prototype%[Symbol.dispose] (§27.1.4.1):
// every iterator-protocol cell inherits a @@dispose that calls
// GetMethod(this, "return") when present and answers undefined.
// Array/Map iterator prototypes define no return (dispose is a
// no-op); an Iterator Helper's return() closes it; `using` drives
// the same face at block exit.
const it: any = [1, 2][Symbol.iterator]();
console.log(typeof it[Symbol.dispose]);
console.log(it[Symbol.dispose]());
console.log(it.next().value);

const h: any = [1, 2, 3].values().map((x: number) => x * 2);
console.log(h.next().value);
console.log(h[Symbol.dispose]());
console.log(h.next().done);

const m = new Map([[1, "a"]]);
const mi: any = m[Symbol.iterator]();
console.log(typeof mi[Symbol.dispose]);
mi[Symbol.dispose]();
console.log(mi.next().done);

function useIt(): void {
  using u: any = [5, 6].values().map((x: number) => x + 1);
  console.log(u.next().value);
}
useIt();
console.log("after-using");
