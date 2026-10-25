// A `for (let …)` head's non-Copy binding whose reference moves into the
// first per-iteration copy (§14.7.4.4): returning the binding itself
// from the first iteration, a loop whose test fails before any
// iteration, and a body that grows an array past its reserved capacity
// while the binding is copied each iteration.
function returnFirst(p: string): string {
  const fs: (() => string)[] = [];
  for (let s = p + "a"; s.length < 9; fs.push(() => s), s = s + "b") return s;
  return "none " + fs.length;
}

type Box = { n: number; tag: string };

function mk(n: number): Box {
  return { n, tag: "t" + n };
}

function zeroIterations(): string {
  const fs: (() => number)[] = [];
  let made = 0;
  for (let o = mk(0); o.n < 0; fs.push(() => o.n), o = mk(1)) made++;
  return made + " " + fs.length;
}

function zeroIterationsMany(): number {
  let total = 0;
  for (let r = 0; r < 2000; r++) {
    const fs: (() => string)[] = [];
    for (let o = mk(r); o.n < 0; fs.push(() => o.tag), o = mk(o.n + 1)) total++;
    total += fs.length;
  }
  return total;
}

function growPastCapacity(): string {
  const xs: string[] = [];
  const fs: (() => string)[] = [];
  for (let s = "x"; s.length < 40; s = s + "y") {
    for (let j = 0; j < 7; j++) xs.push(s + j);
    fs.push(() => s);
  }
  return xs.length + " " + xs[xs.length - 1] + " " + fs.map((h) => h().length).join(",");
}

function growObjects(): string {
  const xs: Box[] = [];
  const fs: (() => number)[] = [];
  for (let o = mk(0); o.n < 30; o = mk(o.n + 1)) {
    xs.push(o, mk(-o.n), o);
    fs.push(() => o.n);
  }
  let sum = 0;
  for (const b of xs) sum += b.n;
  return xs.length + " " + sum + " " + fs.map((h) => h()).join(",");
}

console.log("returnFirst", returnFirst("q"), returnFirst("abcdefghij"));
console.log("zeroIterations", zeroIterations());
console.log("zeroIterationsMany", zeroIterationsMany());
console.log("growPastCapacity", growPastCapacity());
console.log("growObjects", growObjects());
