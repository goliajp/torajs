// a declared function read as a value: its fn-typed params take
// closures however the value is later called
function dbl(x: number): number { return x * 2; }
function run(cb: (x: number) => number): number { return cb(3); }
function hof(f: (g: (x: number) => number) => number): number { return f((x) => x * 10); }

const r = run;
const fs = [run];
console.log(r(dbl), fs[0](dbl), run(dbl), hof(run));

