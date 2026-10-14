// a typed array handed to a slot whose array face holds more than one
// element kind takes values of the other kinds through that slot
function f(xs: (string | number)[] | any[]) { xs.push(3); return xs.length }
const n: number[] = [1, 2];
console.log(f(n));
console.log(n.length, n[2], n[0] + n[2]);
const s: string[] = ["x"];
console.log(f(s));
console.log(s.length, s[1]);
