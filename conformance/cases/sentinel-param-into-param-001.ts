// A parameter reads back what some call site handed it, so a sentinel
// does not stop at the first typed parameter it reaches: `inner`'s
// parameter is armed because `outer` passes its own armed parameter
// on.
const zs: number[] = [1, 2, 3];

function inner(q: number): void {
  console.log("inner", typeof q, q === undefined);
}

function outer(p: number): void {
  console.log("outer", typeof p, p === undefined);
  inner(p);
}

outer(zs[9]);
