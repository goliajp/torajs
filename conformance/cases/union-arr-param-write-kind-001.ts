// a typed array handed to a slot whose array face holds more than one
// element kind takes values of the other kinds through that slot
function f(xs: (string | number)[] | any[]) { xs.push("s"); xs.push(2.5); xs.push({ k: 1 }); xs.push(true); return xs.length }
const n: number[] = [1, 2];
console.log(f(n));
console.log(n.length, n[0] + n[1]);
console.log(n);
const m: number[] = [1.5];
console.log(f(m), m);
const s: string[] = ["x"];
console.log(f(s), s);
const b: boolean[] = [true];
console.log(f(b), b);
