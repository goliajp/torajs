// T-13.5 — exercise head-aware element-walks after shift().
// `xs.shift()` is now O(1) (head_offset++), so subsequent reads
// must add head*8 to slot offsets. This test composes shift with
// every user-visible element-walk path: Index, length, push,
// map, filter, reduce, indexOf, includes, sort, JSON.stringify,
// for-of, pop.

let xs: number[] = [10, 20, 30, 40, 50];
xs.shift();
xs.shift();
// physical: [_, _, 30, 40, 50]; logical: [30, 40, 50]; head=2

console.log(xs.length);            // 3
console.log(xs[0]);                // 30
console.log(xs[1]);                // 40
console.log(xs[2]);                // 50

xs.push(60);
console.log(xs[3]);                // 60
console.log(xs.length);            // 4

let mapped: number[] = xs.map((x: number): number => x * 2);
console.log(mapped[0]);            // 60
console.log(mapped[3]);            // 120

let filtered: number[] = xs.filter((x: number): boolean => x > 35);
console.log(filtered.length);      // 3
console.log(filtered[0]);          // 40

let sum: number = xs.reduce((a: number, b: number): number => a + b, 0);
console.log(sum);                  // 30+40+50+60 = 180

console.log(xs.indexOf(40));       // 1
console.log(xs.indexOf(99));       // -1
console.log(xs.includes(50));      // true

let json: string = JSON.stringify(xs);
console.log(json);                 // [30,40,50,60]

let total: number = 0;
for (let i: number = 0; i < xs.length; i = i + 1) {
  total = total + xs[i];
}
console.log(total);                // 180

let popped: number = xs.pop();
console.log(popped);               // 60
console.log(xs.length);            // 3

let sorted: number[] = [50, 30, 40].sort((a: number, b: number): number => a - b);
console.log(sorted[0]);            // 30
console.log(sorted[1]);            // 40
console.log(sorted[2]);            // 50

// Sort on already-shifted array.
let ys: number[] = [9, 1, 8, 2, 7, 3];
ys.shift();
ys.shift();
ys.sort((a: number, b: number): number => a - b);
console.log(ys[0]);                // 2
console.log(ys[3]);                // 8
