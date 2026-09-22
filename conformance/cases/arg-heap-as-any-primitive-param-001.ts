// A heap value passed through `as any` into a primitive parameter.
// `x as any` lowers to x's own representation, so the argument used to
// arrive as its raw pointer: a number parameter printed the address,
// a string parameter read the array as a string block. The shapes here
// print the same whether the value converts or passes through.
function num(n: number) { return String(n); }
function str(x: string) { return "<" + x + ">"; }
function bool(x: boolean) { return x ? "T" : "F"; }
console.log(num([4] as any), str([1, 2] as any), bool([] as any), bool({} as any));
const arrow = (x: string) => "[" + x + "]";
console.log(arrow(["ab", "c"] as any));
class P { show(n: number) { return String(n); } }
console.log(new P().show([7] as any));
let total = 0;
for (let i = 0; i < 100000; i++) { total += str([i % 10] as any).length; }
console.log(total);
