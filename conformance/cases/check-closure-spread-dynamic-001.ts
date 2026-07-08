// closure-value callee spread (chunk 685)
const f = (a: number, b: number): number => a + b;
const arr: number[] = [40, 2];
console.log(f(...arr));
// capturing closure
const base: number = 100;
const g = (a: number, b: number): number => base + a + b;
console.log(g(...arr));
// prefix + spread
const one: number[] = [2];
console.log(f(40, ...one));
// string lane
const s = (x: string, y: string): string => x + y;
const sa: string[] = ["a", "b"];
console.log(s(...sa));
// extra elements ignored
const big: number[] = [1, 2, 3, 4];
console.log(f(...big));
