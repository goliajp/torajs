// a typed array handed to a slot whose array face holds more than one
// element kind takes values of the other kinds through that slot
function g(xs: number[] | string[]) { (xs as any[]).push("w"); return xs.length }
const k: number[] = [3];
console.log(g(k), k);
