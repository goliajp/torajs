// A boolean[] hole (a write past the end, a longer length) holds the
// word 2 — neither false nor true — under a shadow mark: a read answers
// undefined, indexOf / includes tell it from false, JSON writes null.
const b = [true];
b[2] = false;
console.log(b, 1 in b, b[1], b.length);
console.log(b.filter((x) => x), b.indexOf(false), b.includes(undefined as any));
const c = [true, false];
c.length = 4;
console.log(c, c.indexOf(false), c.includes(undefined as any), c.lastIndexOf(false), c.includes(false));
const d = [false];
d[2] = true;
let m = 0;
for (const v of d) {
  if (v === undefined) m++;
}
console.log(m, JSON.stringify(d), d.join(","), d.some((x) => x === false));
const e = [true, false];
e[3] = true;
e.reverse();
console.log(e);
console.log(e.sort());
const i = [1, 0];
i[4] = 5;
console.log(i, 2 in i, i[2]);
