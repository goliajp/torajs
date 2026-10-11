// Every typed reader of a declared field reads it through [[Get]], so a
// getter Object.defineProperty installs over the field runs for each:
// Reflect.get (§28.1.6), an object spread (§7.3.25 CopyDataProperties),
// JSON.stringify (§25.5.2.4), Object.values / Object.entries (§20.1.2.22)
// and a call through a callable field (§13.3.6.1).
class Cell {
  v: number = 1;
  s: string = "a";
  f: () => number = () => 10;
}
let reads = 0;
const c = new Cell();
Object.defineProperty(c, "v", {
  get() {
    reads++;
    return 7;
  },
});
Object.defineProperty(c, "s", {
  get() {
    reads++;
    return "got";
  },
});
console.log(Reflect.get(c, "v"), Reflect.get(c, "s"), reads);
const copy = { ...c };
console.log(copy.v, copy.s, reads);
console.log(JSON.stringify(c), reads);
console.log(Object.values(c).length, Object.values(c)[0], Object.values(c)[1], reads);
console.log(JSON.stringify(Object.entries(c).slice(0, 2)), reads);
const own = c.f;
let calls = 0;
Object.defineProperty(c, "f", {
  get() {
    calls++;
    return own;
  },
});
console.log(c.f(), c.f(), calls);
const plain = new Cell();
console.log(Reflect.get(plain, "v"), { ...plain }.s, JSON.stringify(plain), plain.f());
