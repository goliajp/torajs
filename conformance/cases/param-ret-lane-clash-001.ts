// A function's first parameter and its return value want the same ABI
// lane — x0 for integers and pointers, d0 for floats. Both are parked
// there directly when nothing in the body calls out, and a body that
// reads its parameter after the result is first written then reads the
// result instead: `count = 0` lands on top of `parents`, and the next
// element read dereferences 0.
//
// It takes a body with no calls of its own (a call would move the
// parameter off its caller-saved lane) and enough call sites that the
// body is compiled on its own rather than inlined into each one.

function leaf_count(parents: number[]): number {
  let count = 0;
  let n = parents.length;
  for (let i: number = 0; i < n; i = i + 1) {
    let has_child: boolean = false;
    for (let j: number = 0; j < n; j = j + 1) {
      if (parents[j] === i) { has_child = true; break; }
    }
    if (!has_child) { count = count + 1; }
  }
  return count;
}

// the float element class is what puts the comparison on the FP side
// and leaves the integer lane under pressure
const r: number[] = [-1, 0, 0, 1, 1, 2, 5];
r.find((x: number): boolean => x > 4);

console.log(leaf_count(r));
console.log(leaf_count(r));
console.log(leaf_count(r));
console.log(leaf_count(r));
console.log(leaf_count(r));
console.log(leaf_count(r));
console.log(leaf_count(r));

// the float lane, same shape: `acc` is the result, `base` the first
// parameter, and `base` is read after `acc` has been written
function over(base: number, xs: number[]): number {
  let acc = 0.5;
  let n = xs.length;
  for (let i: number = 0; i < n; i = i + 1) {
    if (xs[i] > base) { acc = acc + xs[i]; }
  }
  return acc;
}

const s: number[] = [1.5, 2.5, 3.5, 4.5];
console.log(over(2, s));
console.log(over(2, s));
console.log(over(2, s));
console.log(over(2, s));
console.log(over(2, s));
console.log(over(2, s));
console.log(over(2, s));
