// An object literal's method whose fn-typed parameter is handed a
// function of another face answers what each value handed to it
// returns: a closure declared to return `any` that returns a number
// keeps the sum a number, one that returns a string makes it a string.
function mkA(k: number): (y?: number) => any { return (y?: number) => (y === undefined ? 5 : y) * k }
function mkS(s: string): (y?: number) => any { return (y?: number) => s + (y ?? "") }
const obj = {
  m(f: (y?: number) => number) { return f() + f(3) },
};
console.log(obj.m(mkA(2)), obj.m((y?: number) => 1));
const box = {
  m(f: (y?: number) => number) { return f() + f(4) },
};
console.log(box.m(mkS("s")), typeof box.m(mkS("t")), box.m(mkA(1)));
const other = { m: (n: number) => n * 3 };
console.log(other.m(2));
