// unshift / splice writes through a cast any view of a typed array.
const c: number[] = [3, 1, 2];
(c as any).unshift("head");
console.log(c.length, c[0], c[1]);
console.log(c);

const d: number[] = [3, 1, 2];
(d as any).splice(1, 0, "q", undefined);
console.log(d.length);
console.log(d);
const removed = (d as any).splice(0, 2, { z: 1 });
console.log(removed);
console.log(d);

const e: number[] = [1.5, 2.5];
(e as any).unshift(null, true);
console.log(e.length, e[2] + e[3]);
console.log(e.indexOf(2.5));
