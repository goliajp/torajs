// float_demote's post-op growth guard checks an add/sub after the i64
// op ran. With a constant operand near 2^63 the i64 op itself wraps,
// so the checked result is not the sum: the side exit must not write
// that result back into the f64 loop. Every function below has to
// print what bun prints. One function per loop keeps each value in a
// region of its own.

// the i64 add wraps past i64::MAX on the first iteration
function addBig(n: number): string {
  let hit = 0;
  let last = 0;
  for (let i = 0; i < n; i++) {
    const x = (i + 5000) + 9223372036854774784;
    if (x > 0) hit++;
    last = x;
  }
  return hit + " " + last;
}
console.log(addBig(100));

// the constant on the left
function addBigLeft(n: number): string {
  let hit = 0;
  let last = 0;
  for (let i = 0; i < n; i++) {
    const x = 9223372036854774784 + (i + 5000);
    if (x > 0) hit++;
    last = x;
  }
  return hit + " " + last;
}
console.log(addBigLeft(100));

// the i64 sub wraps past i64::MIN
function subBig(n: number): string {
  let hit = 0;
  let last = 0;
  for (let i = 0; i < n; i++) {
    const x = -9223372036854774784 - (i + 5000);
    if (x < 0) hit++;
    last = x;
  }
  return hit + " " + last;
}
console.log(subBig(100));

// subtracting a large negative constant is an add that wraps
function subNegBig(n: number): string {
  let hit = 0;
  let last = 0;
  for (let i = 0; i < n; i++) {
    const x = (i + 5000) - -9223372036854774784;
    if (x > 0) hit++;
    last = x;
  }
  return hit + " " + last;
}
console.log(subNegBig(100));

// a constant just inside 2^53: the post exit stays exact
function addEdge(n: number): string {
  let hit = 0;
  let last = 0;
  for (let i = 0; i < n; i++) {
    const x = (i + 5000) + 9007199254740991;
    if (x > 9007199254740991) hit++;
    last = x;
  }
  return hit + " " + last;
}
console.log(addEdge(100));

// the large value comes in as an argument; a literal argument may
// still reach the loop as a constant
function addVar(n: number, big: number): string {
  let hit = 0;
  let last = 0;
  for (let i = 0; i < n; i++) {
    const x = (i + 5000) + big;
    if (x > 0) hit++;
    last = x;
  }
  return hit + " " + last;
}
console.log(addVar(100, 9223372036854774784));
console.log(addVar(100, -9223372036854774784));

// both sides variables: the large value is only known at run time
function addRt(n: number, big: number): string {
  let hit = 0;
  let last = 0;
  for (let i = 0; i < n; i++) {
    const x = (i + 5000) + big;
    if (x > 0) hit++;
    last = x;
  }
  return hit + " " + last;
}
const bigs = [Number("9223372036854774784"), Number("-9223372036854774784")];
console.log(addRt(100, bigs[0]));
console.log(addRt(100, bigs[1]));
