// A this-reading function expression as an iterator-helper callback.
//
// §27.1.4's consumers all invoke their callback with
// Call(cb, undefined, « value, counter »), so a fn-expr body reading
// `this` sees undefined. The body used to be a compile error
// ("closure references unknown identifier __this"): the receiver
// channel (FLAG_CLOSURE_RECV_FIRST) existed for the array-HOF family
// but the iterator-helper kernels never read the flag, and the
// promoter never admitted the generator-object receiver shape.
//
// The receiver test is syntactic — `let it = g()` where `g` is a
// declared `function*` (by this pass's time, the desugared factory
// whose return type names the synthesized `__Gen_*` class).

function* g() {
  yield 1;
  yield 2;
}

// eager consumers: every / some / find / forEach / reduce
let it1 = g();
console.log(
  it1.every(function (v: any, c: any) {
    console.log("every-this=" + String(this), v, c);
    return v < 5;
  }),
);

let it2 = g();
console.log(
  it2.some(function (v: any) {
    return String(this) === "undefined" && v === 2;
  }),
);

let it3 = g();
console.log(
  it3.find(function (v: any) {
    return this === undefined && v > 1;
  }),
);

let it4 = g();
it4.forEach(function (v: any) {
  console.log("forEach", String(this), v);
});

// reduce has a callback here, unlike the array family's loud reject
let it5 = g();
console.log(
  it5.reduce(function (acc: any, v: any) {
    return acc + v + (this === undefined ? 0 : 100);
  }, 0),
);

// lazy mints: map / filter / flatMap drive the same flag through
// their per-step invoke
let it6 = g();
console.log(
  it6
    .map(function (v: any) {
      return String(this) + ":" + v;
    })
    .toArray()
    .join("|"),
);

let it7 = g();
console.log(
  it7
    .filter(function (v: any) {
      return this === undefined && v % 2 === 0;
    })
    .toArray()
    .join(","),
);

// a plain (this-free) callback keeps the unshifted path
let it8 = g();
console.log(
  it8
    .map(function (v: any) {
      return v * 10;
    })
    .toArray()
    .join(","),
);
