// the annotation an `any` value is cast to is the one of the binding
// it is written to, found in that binding's own scope: a function's
// own `let` or parameter, a method's parameter, a binding a closure
// captures as declared where the closure is made; a binding of the
// same name in another function is another binding and its annotation
// is never read, and a binding without one is not cast
//
// departures from bun, checked against the .expected: an annotated slot
// holds what its annotation says, so a class slot given an object of no
// class, and an object-type slot given a borrowed dynamic object, throw
// TypeError at the write

class P {
  x: number;
  constructor(x: number) {
    this.x = x;
  }
}
type Q = { x: number };

// declared first: a name lookup across the program would find these
function first(): void {
  let q: P = new P(7);
  console.log("first", q.x);
}
first();
function second(): void {
  let p: Q = { x: 8 };
  console.log("second", p.x);
}
second();

// a captured `Q` slot takes a fresh object as a rebuilt cell, which a
// `P` slot would refuse
function captured(text: string): number {
  let q: Q = { x: 1 };
  const set = (): void => {
    q = JSON.parse(text);
  };
  set();
  return q.x;
}
console.log("captured", captured('{"x":2.5}'), captured('{"x":-4}'));

// a captured slot without an annotation holds what it is given
function inferred(o: any): number {
  let r = { x: 1 };
  const set = (): void => {
    r = o;
  };
  set();
  return r.x;
}
console.log("inferred", inferred({ x: 3, y: 4 }), inferred({ x: 2.5 }));

// a method parameter is cast to its own annotation, `P`, not to the
// `Q` of the `p` above, and the throw reaches the caller's handler
class K {
  m(p: P, o: any): number {
    p = o;
    return p.x;
  }
}
const k = new K();
console.log("method", k.m(new P(1), new P(5)));
try {
  console.log("method", k.m(new P(1), JSON.parse('{"x":2.5}')));
} catch (e) {
  console.log("method threw", (e as Error).message);
}

// a function parameter likewise: `Q`, not the `P` of the `q` above
function param(q: Q, text: string): number {
  q = JSON.parse(text);
  return q.x;
}
console.log("param", param({ x: 1 }, '{"x":4.5}'));
try {
  console.log("param", param({ x: 1 }, '{"x":"s"}'));
} catch (e) {
  console.log("param threw", (e as Error).message);
}
console.log("end");
