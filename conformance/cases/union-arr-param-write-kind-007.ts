// a typed array handed to a slot whose array face holds more than one
// element kind takes values of the other kinds through that slot
function f(xs: (string | number)[]) { xs.push("s"); return xs.length }
const n: number[] = [1, 2];
console.log(f(n));
console.log(n.length, n[0] + n[1], n[2]);
console.log(n);
function g(xs: number[] | undefined) { (xs as any).push("q"); return xs!.length }
const m: number[] = [4];
console.log(g(m), m);
