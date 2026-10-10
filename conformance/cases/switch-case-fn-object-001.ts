// a switch selects with IsStrictlyEqual, so a named function as a case
// value, or as the scrutinee, is its one fn object — directly or
// through a const binding of it — and matches an any holding it
function inc(n: number): number { return n + 1; }
function dec(n: number): number { return n - 1; }
const g: any = inc;
const h = inc;
function pick(v: any): string {
  switch (v) {
    case dec: return "dec";
    case inc: return "inc";
    default: return "other";
  }
}
let hits = 0;
for (let i = 0; i < 10000; i++) {
  switch (g) {
    case h: hits++; break;
    default: break;
  }
  switch (inc) {
    case g: hits++; break;
    default: break;
  }
}
console.log(pick(inc), pick(dec), pick(g), pick(1), hits);
