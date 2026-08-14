// An accessor on a capturing nested class lowers to
// `Object.defineProperty(K.prototype, …)`, which is also where its
// attributes come from: §15.7.14 makes a class accessor configurable
// and NOT enumerable, and a fresh `defineProperty` gives it exactly
// that. A getter and a setter of the same name are two members and so
// two calls — the second keeps the first half.
function run(base: number) {
  class Cell {
    n: number;
    constructor(start: number) {
      this.n = start + base;
    }
    get double() {
      return this.n * 2;
    }
    set double(v: number) {
      this.n = v / 2;
    }
    get label() {
      return "n=" + this.n;
    }
  }
  const c = new Cell(1);
  const out: any[] = [];
  out.push(c.double);
  c.double = 10;
  out.push(c.n);
  out.push(c.label);
  const seen: string[] = [];
  for (const k in c as any) seen.push(k);
  out.push("[" + seen.join("|") + "]");
  return out.join(",");
}

console.log(run(3));
console.log(run(30));
