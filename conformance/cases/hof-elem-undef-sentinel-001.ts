// A HOF kernel loads an element and hands it to the callback at the
// element's own width. Nothing told the callback the value can be the
// out-of-range sentinel, so `typeof x` inside answered "number" for an
// `undefined` that `typeof ys[0]` at the same index answered right.
const zs: number[] = [];

const pushed: number[] = [];
pushed.push(zs[9]);
console.log("index", typeof pushed[0], pushed[0] === undefined);
pushed.forEach((x) => console.log("forEach", typeof x, x === undefined));
pushed.map((x) => {
  console.log("map", typeof x, x === undefined);
  return 0;
});
console.log("some", pushed.some((x) => typeof x === "undefined"));

const written: number[] = [0];
written[0] = zs[9];
written.filter((x) => {
  console.log("filter", typeof x, x === undefined);
  return true;
});

const literal: number[] = [zs[9]];
literal.find((x) => {
  console.log("find", typeof x, x === undefined);
  return true;
});

const filled: number[] = [1, 2];
filled.fill(zs[9]);
filled.every((x) => {
  console.log("every", typeof x, x === undefined);
  return true;
});

// reduce hands the element as parameter 1.
pushed.reduce((acc, x) => {
  console.log("reduce elem", typeof x, x === undefined);
  return acc;
}, 0);

// sort is deliberately absent: §23.1.3.30 step 3 sorts undefined
// elements to the end WITHOUT calling the comparator, so its
// parameters can never see one.

// A container never written a sentinel keeps the plain number lane.
const clean: number[] = [];
clean.push(4);
clean.forEach((x) => console.log("clean", typeof x, x + 1));
