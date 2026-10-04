// The pre-reserved fill loop addresses each append from slot[0]'s
// address, computed once above the loop. Every append has to land at
// the array's current length whatever that length was on entry, and
// the slots already there must survive.

const n = parseInt(process.argv[2] ?? "7", 10) || 7;

// starts non-empty: the first append goes after the literal's slots
const xs: number[] = [100, 200, 300];
for (let i = 0; i < n; i++) {
  xs.push(i * 3);
}
console.log(xs.length, xs[0], xs[2], xs[3], xs[xs.length - 1], xs.join(","));

// two appends a trip, to two arrays in lockstep, one of them non-empty
const a: number[] = [-1];
const b: number[] = [];
for (let i = 0; i < n; i = i + 1) {
  a.push(i);
  a.push(-i);
  b.push(i + 0.5);
}
console.log(a.length, a.join(","), b.length, b.join(","));

// a second fill of the same array starts where the first one stopped
const c: number[] = [];
for (let i = 0; i < 3; i++) {
  c.push(i);
}
for (let i = 0; i < n; i++) {
  c.push(10 + i);
}
let sum = 0;
for (let i = 0; i < c.length; i++) {
  sum = sum + c[i];
}
console.log(c.length, c.join(","), sum);

// a zero-trip fill leaves the array as it was
const d: number[] = [5, 6];
const none = parseInt("-3", 10);
for (let i = 0; i < none; i++) {
  d.push(i);
}
console.log(d.length, d.join(","));

// strings are refcounted slots: the address change must not lose one
const s: string[] = ["head"];
for (let i = 0; i < n; i++) {
  s.push("s" + String(i));
}
console.log(s.length, s.join("|"));
