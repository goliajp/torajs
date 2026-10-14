// a typed array handed to a slot whose array face holds more than one
// element kind takes values of the other kinds through that slot
function f(xs: (string | number)[] | any[]) { xs.unshift("u"); xs[5] = "z"; xs[1] = { k: 2 }; return xs.length }
const n: number[] = [1, 2];
console.log(f(n));
console.log(n.length);
console.log(n);
function e(xs: (string | number)[]) { xs[0] = "e"; xs.splice(1, 0, "p"); xs.fill("f", 3); return xs.length }
const p: number[] = [7, 8, 9, 10];
console.log(e(p), p);
