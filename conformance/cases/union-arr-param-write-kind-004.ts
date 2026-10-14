// a typed array handed to a slot whose array face holds more than one
// element kind takes values of the other kinds through that slot
const n: number[] = [1];
const u: (string | number)[] = n;
u.push("u");
console.log(n.length, n[1], n);
type H = { xs: (string | number)[] | undefined };
const m: number[] = [2];
const h: H = { xs: m };
h.xs!.push("h");
console.log(m.length, m);
function back(a: number[]): (string | number)[] { return a }
const r: number[] = [3];
back(r).push("r");
console.log(r);
const s: string[] = ["s"];
const o: { v: (string | number)[] | any[] } = { v: s };
o.v.push(9);
console.log(s);
