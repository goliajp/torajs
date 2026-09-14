// §10.1.8.1 [[Get]] on a class instance under an integer key: an accessor the
// program defined onto the instance (`Object.defineProperty(c, 0, { get })`) is
// invoked by the integer-index read the same way the string spelling invokes it
// (§7.1.19 ToPropertyKey makes `c[0]` ≡ `c["0"]`).
class C {
  n: number;
  constructor() {
    this.n = 1;
  }
}
let reads = 0;
function g(this: any): number {
  reads++;
  return this.n * 10;
}
const c: any = new C();
Object.defineProperty(c, 0, { get: g, configurable: true });
Object.defineProperty(c, "1", { get: () => 7, configurable: true });
console.log(c[0], c["0"], c[1], c["1"], reads);
const k: any = 0;
console.log(c[k], Reflect.get(c, 0), reads);
c[2] = 5;
console.log(c[2], c["2"]);
console.log(JSON.stringify(Object.getOwnPropertyNames(c)));
console.log("done");
