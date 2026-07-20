// Arr<Any> receiver: splice(start, deleteCount, ...items) direct
// call — mixed item shapes NaN-box into the 8-byte slots (RFC
// 20260720-splice-insert follow-up).
const xs: any[] = [1, "two", true];
const removed = xs.splice(1, 1, 42, "s", "heap-string-long-enough", 2.5, false, null, undefined, [7, 8]);
console.log(xs.length);
console.log(removed.length);
console.log(removed[0]);
for (const v of xs) {
  console.log(v);
}

// borrow shapes: any-typed local + concrete scalar local
const av: any = "borrowed-any-value";
const n = 5;
xs.splice(0, 0, av, n);
console.log(xs[0]);
console.log(xs[1]);
console.log(av);

// owned any temp (call result) transfers its ref into the slot
function mk(): any {
  return [1, 2, 3];
}
xs.splice(2, 0, mk());
console.log(xs[2].length);

// insert past the end + negative start
xs.splice(-1, 0, "tail-1", "tail-2");
console.log(xs[xs.length - 3]);
console.log(xs[xs.length - 2]);

// toSpliced (knife-3 sibling shares the emit): source untouched
const base: any[] = ["a", 1, null];
const ts = base.toSpliced(1, 1, "mid", 2.5, true);
console.log(base.length);
console.log(ts.length);
for (const v of ts) {
  console.log(v);
}
