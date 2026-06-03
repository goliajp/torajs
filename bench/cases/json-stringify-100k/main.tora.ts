// Inline the record build — tora's check pass needs an explicit return
// type for object-returning functions, so we skip the helper to keep
// the bench surface what every other comparator is doing inline.
let total: number = 0;
let n: number = 100000;
for (let i: number = 0; i < n; i = i + 1) {
  let r = { id: i, name: "row", score: i * 7, active: (i & 1) === 0 };
  let s: string = JSON.stringify(r);
  total = total + s.length;
}
console.log(total);
