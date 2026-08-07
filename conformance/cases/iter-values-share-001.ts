// An iterator yielding a heap element hands out a reference of its
// own — the container keeps its stake.
//
// The `.value` box adopts whatever payload the step function gives it,
// but the slot read behind `values()` / `keys()` is a borrow. Forward
// the payload without a `+1` and the box and the container both think
// they own the single reference; the second release frees the element
// while the container still points at it. It reads as data loss rather
// than a crash — the container answers `undefined` where the element
// used to be.
//
// Five steps, because the first release is what frees it: the damage
// only shows on a later read.

const arr: any[] = [[1], [2], [3]];
for (let i = 0; i < 5; i++) {
  const v = arr.values().next().value;
}
console.log(arr[0][0], arr[1][0], arr[2][0]);
console.log(JSON.stringify(arr));

// same through a hoisted iterator, restarted each round
for (let i = 0; i < 5; i++) {
  const it = arr.values();
  const v = it.next().value;
}
console.log(JSON.stringify(arr));

// Map values + keys, both heap
const m = new Map<any, any>([[["k"], [9]]]);
for (let i = 0; i < 5; i++) {
  const v = m.values().next().value;
  const k = m.keys().next().value;
}
console.log(JSON.stringify(Array.from(m.entries())));

// Set elements
const s = new Set<any>([[8], [7]]);
for (let i = 0; i < 5; i++) {
  const v = s.values().next().value;
}
console.log(JSON.stringify(Array.from(s)));

// for-of borrows the same way
for (let i = 0; i < 5; i++) {
  for (const v of arr) {
    // just touch it
  }
}
console.log(JSON.stringify(arr));

// entries() pairs keep the element alive too
for (let i = 0; i < 5; i++) {
  const pair = arr.entries().next().value;
}
console.log(JSON.stringify(arr));
