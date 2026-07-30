// RFC 20260730-iterator-global 刀 3 — eager consumers: forEach /
// some / every / find / reduce over the iterator-helper substrate.

function* nums() {
  yield 1;
  yield 2;
  yield 3;
  yield 4;
}

// forEach with (value, counter).
const acc: any[] = [];
const it1: any = nums();
it1.forEach((v: any, i: any) => {
  acc.push(v * 10 + i);
});
console.log(JSON.stringify(acc));

// some / every short-circuit.
const it2: any = nums();
console.log(it2.some((v: any) => v === 3));
const it3: any = nums();
console.log(it3.every((v: any) => v < 3));
const it4: any = nums();
console.log(it4.every((v: any) => v >= 1));

// find answers the item (and undefined on a miss).
const it5: any = nums();
console.log(it5.find((v: any) => v > 2));
const it6: any = nums();
console.log(it6.find((v: any) => v > 99));

// reduce with and without an initial value.
const it7: any = nums();
console.log(it7.reduce((a: any, v: any) => a + v, 100));
const it8: any = nums();
console.log(it8.reduce((a: any, v: any) => a + v));

// reduce of empty with no initial throws TypeError.
function* empty() {}
try {
  const it9: any = empty();
  it9.reduce((a: any, v: any) => a + v);
  console.log("no-throw");
} catch (e) {
  console.log(e instanceof TypeError);
}

// eager on a lazy chain.
const it10: any = nums();
console.log(it10.map((v: any) => v * 2).reduce((a: any, v: any) => a + v, 0));
