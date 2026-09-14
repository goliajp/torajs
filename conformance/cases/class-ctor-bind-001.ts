// Function.prototype.bind over a CLASS constructor: tr models the
// class object as a dynobj, and `.bind` on it fell through to "value
// is not a function". The bound cell now holds the class as its
// target — `new` constructs it with boundArgs ++ args (§10.4.1.2),
// bound-over-bound chains concatenate outside-in, `instanceof` sees
// the class, and a plain call is the class's own no-`new` TypeError.
class Coll {
  n: number; k: string;
  constructor(n?: number, k?: string) { this.n = n === undefined ? -1 : n; this.k = k === undefined ? "" : k; }
}
const B: any = (Coll as any).bind(null, 7);
console.log("typeof", typeof B);
const a: any = new B("z");
console.log("new", a instanceof Coll, a.n, a.k);
const BB: any = B.bind({}, "q");
const b: any = new BB();
console.log("nested", b instanceof Coll, b.n, b.k);
try { B(); console.log("call", "no throw"); } catch (e: any) { console.log("call", e instanceof TypeError); }
const T: any = Coll.bind(null);
const t: any = new T(3, "w");
console.log("typed", t instanceof Coll, t.n, t.k);
async function viaFromAsync(): Promise<void> {
  const d = await (Array.fromAsync as any).call(Coll.bind(null), { length: 1, 0: 9 });
  console.log("fromAsync", d instanceof Coll, d.n, d[0]);
}
viaFromAsync();
