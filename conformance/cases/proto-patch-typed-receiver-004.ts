// RFC 20260806 — the ordering guarantee, and the two shapes a
// callback can arrive in, on a typed receiver whose family stands
// down wholesale.
//
// `Array.prototype[k] = v` under a computed key cannot be attributed
// to one method, so the whole Array family stands down. That is the
// shape test262's array-like `this` cases take, and it is what makes
// every call below go through the runtime dispatcher.

function named(v, i, o) {
  return true;
}

function annotated(v: any, i: any, o: any): boolean {
  return true;
}

const key: string = "2";
(Array.prototype as any)[key] = 9;

const xs: number[] = [4, 5];

// Named declaration, annotated declaration, and an arrow literal must
// all reach the same dispatcher and agree.
console.log(xs.every(named), xs.every(annotated), xs.every((v: any) => true));

(Array.prototype as any).every = function () {
  return "PATCHED";
};

console.log(xs.every(named), xs.every(annotated), xs.every((v: any) => true));

// A call sequenced before a later patch still answers from the kernel:
// the decision is the runtime bitmap's, not the compiler's.
console.log(xs.join("-"));
(Array.prototype as any).join = function () {
  return "PATCHED-join";
};
console.log(xs.join("-"));

// A delete is the same channel as a write: the method is gone, so the
// call is a TypeError rather than a fall-back to the kernel.
delete (Array.prototype as any).join;
try {
  console.log(xs.join("-"));
} catch (e) {
  console.log("caught after delete");
}
