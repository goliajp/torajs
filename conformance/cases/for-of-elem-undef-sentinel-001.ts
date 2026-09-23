// A for-of loop variable off a container some write filled with the
// out-of-range sentinel can hold one, and the body reads it by name.
// `for (const x of ys) typeof x` answered "number" where the identical
// `typeof ys[i]` at the same index answered "undefined".
const zs: number[] = [];

const ys: number[] = [];
ys.push(zs[9]);
for (let i = 0; i < ys.length; i++) {
  console.log("index", typeof ys[i], ys[i] === undefined);
}
for (const x of ys) {
  console.log("for-of", typeof x, x === undefined);
}

const lit: number[] = [zs[9]];
for (const x of lit) {
  console.log("literal", typeof x, x === undefined);
}

// A container never written a sentinel keeps the plain number lane.
const clean: number[] = [];
clean.push(4);
for (const x of clean) {
  console.log("clean", typeof x, x + 1);
}
