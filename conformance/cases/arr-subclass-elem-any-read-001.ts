// An array element that is an instance of a Map or Date subclass,
// read through the any lane (an iterator step, an index read, `at`,
// slice, Object.values). The reads that copy a substring view out of a
// split product tested the view bit without the cell's tag, and that
// bit is the subclass mark on a Map, Set, Date, RegExp or Promise: the
// instance was "copied" as if it were a string, and using it crashed.
class M extends Map<string, number> {
  tag(): string {
    return "M";
  }
}
class D extends Date {
  tag(): string {
    return "D";
  }
}
const m = new M([["a", 1]]);
const d = new D(0);
const xs: any = [m, d];
for (const x of xs) console.log(x.tag(), x === m || x === d);
console.log(xs[0] === m, xs.at(1) === d, xs[Symbol.iterator]().next().value === m);
console.log(xs.slice(0)[0] === m, Object.values(xs)[1] === d);
