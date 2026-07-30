// RFC 20260730-iterator-global 刀 2b — filter / take / drop / toArray
// over the IterHelper substrate, chained and single-shot.

function* nums() {
  yield 1;
  yield 2;
  yield 3;
  yield 4;
  yield 5;
}

// filter: predicate (value, counter), lazy.
let seen = 0;
const it1: any = nums();
const odd: any = it1.filter((v: any, i: any) => {
  seen++;
  return v % 2 === 1;
});
console.log(seen);
for (const x of odd) {
  console.log(x);
}
console.log(seen);

// take: limit, closes underlying at the boundary.
const it2: any = nums();
const first2: any = it2.take(2);
for (const x of first2) {
  console.log(x);
}
console.log(JSON.stringify(first2.next()));

// take(0) is done immediately.
const it3: any = nums();
const none: any = it3.take(0);
console.log(JSON.stringify(none.next()));

// drop: skips ahead once.
const it4: any = nums();
const rest: any = it4.drop(3);
for (const x of rest) {
  console.log(x);
}

// toArray: eager collector, on a helper and on a builtin iter cell.
const it5: any = nums();
const doubled: any = it5.map((v: any) => v * 2);
console.log(JSON.stringify(doubled.toArray()));
const av: any = ["x", "y"].values();
console.log(JSON.stringify(av.toArray()));

// filter + take + toArray stacked in one expression.
const it6: any = nums();
const out: any = it6.filter((v: any) => v > 1).take(2).toArray();
console.log(JSON.stringify(out));

// RangeError on a negative limit.
try {
  const it7: any = nums();
  it7.take(-1);
  console.log("no-throw");
} catch (e) {
  console.log(e instanceof RangeError);
}
