// some / every skip the holes of a TYPED source (§23.1.3.29 / §23.1.3.6
// HasProperty step); find / findIndex visit them (§23.1.3.9 has no
// such step).
let y = [1.5, 2.5];
y[4] = 3.5;
let seen: number[] = [];
console.log(y.some((v, i) => { seen.push(i); return false; }), seen);
seen = [];
console.log(y.every((v, i) => { seen.push(i); return true; }), seen);
console.log(y.some((v) => v === 0), y.every((v) => v > 1));
let g = [1, 2];
g.length = 4;
console.log(g.every((v) => v > 0), g.some((v) => v !== 1 && v !== 2));
let e: number[] = [];
e.length = 3;
console.log(e.some(() => true), e.every(() => false));
const s = ["a"];
s[2] = "c";
seen = [];
console.log(s.every((v, i) => { seen.push(i); return v.length === 1; }), seen);
const z = [1, 2, 3];
console.log(z.some((v) => v > 2), z.every((v) => v > 0));
