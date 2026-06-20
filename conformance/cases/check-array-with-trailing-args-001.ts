// S283 — Array.prototype.with(index, value, ...trailing) trailing-arg
// ignore per ES §23.1.3.39. Spec reads only (index, value); tora's
// arr_with intrinsic is 2-arg only. Same S272 family — trailing args
// must eval-then-discard, not silent-drop.

let calls = 0;
function step<T>(v: T): T {
    calls = calls + 1;
    return v;
}

const xs: number[] = [1, 2, 3, 4, 5];

// Baseline — 2-arg form
const a = xs.with(0, 99);
console.log(a.length);
console.log(a[0]);
console.log(a[4]);

// Trailing widen — 3-arg
const b = xs.with(1, 88, step(7));
console.log(b.length);
console.log(b[1]);
console.log("calls after 3-arg:", calls);

// Trailing widen — 4-arg
const c = xs.with(2, 77, step(8), step(9));
console.log(c.length);
console.log(c[2]);
console.log("calls after 4-arg:", calls);

// Trailing widen — 5-arg
const d = xs.with(3, 66, step(10), step(11), step(12));
console.log(d.length);
console.log(d[3]);
console.log("calls after 5-arg:", calls);

// Source unchanged
console.log(xs[0]);
console.log(xs[4]);
