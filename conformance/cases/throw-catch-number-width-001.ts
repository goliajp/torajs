// A `throw` and the `catch (e: number)` that receives it have to agree
// on how wide the value is.
//
// There is one pending-throw slot in the runtime and it holds raw 8
// bytes: the throw site encodes into it, the catch binding decodes back
// out. Nothing joined the two, so `throw xs[i]` on a widened array
// wrote f64 bits that a catch binding compiled as an integer read
// straight through — silently, and exit 0.
//
// Joining them means every numeric throw site in a program shares one
// class, so an integer throw widens to match a fractional one rather
// than writing bits the other end would misread. That is the
// conservative direction: a JS number IS an f64.

function first_neg(xs: number[]): number {
  for (let i: number = 0; i < xs.length; i = i + 1) {
    if (xs[i] < 0) {
      throw xs[i];
    }
  }
  return 0;
}

const arr: number[] = [1, 2, -3, 4];
arr[0] = 1.5;
let caught: number = 0;
try {
  first_neg(arr);
} catch (e: number) {
  caught = e;
}
console.log(caught);

// a fractional value all the way through
const frac: number[] = [1, -2.5];
let c2: number = 0;
try {
  first_neg(frac);
} catch (e: number) {
  c2 = e;
}
console.log(c2);

// an integer throw sharing the program with the fractional ones — this
// is the side that breaks if only one end is taught the width
function pick(xs: number[], i: number): number {
  if (i === 0) {
    throw 7;
  }
  throw xs[i];
}
let a: number = 0;
try {
  pick(arr, 2);
} catch (e: number) {
  a = e;
}
let b: number = 0;
try {
  pick(arr, 0);
} catch (e: number) {
  b = e;
}
console.log(a, b);

// a plain integer throw with no array in sight
function boom(): number {
  throw 42;
}
let c: number = 0;
try {
  boom();
} catch (e: number) {
  c = e;
}
console.log(c);

// the any-typed catch reads the tag, and still sees a number
try {
  first_neg(arr);
} catch (e) {
  console.log(typeof e, e);
}

// a string throw is untouched by any of this
try {
  throw "boom";
} catch (e: string) {
  console.log(e);
}

// rethrow through an outer handler
let outer: number = 0;
try {
  try {
    first_neg(arr);
  } catch (e: number) {
    throw e;
  }
} catch (e: number) {
  outer = e;
}
console.log(outer);
