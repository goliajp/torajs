// §13.3.8 `Arguments` is ONE production, shared by a call, a `new`,
// and a `super` call — so ES2017's trailing comma is legal in all
// three. It was written out three times in the parser and only the
// call site had the break, which made `new Error("x",)` a parse
// error while `f("x",)` was fine.
console.log(new Error("m",).message);
console.log(new Map([[1, 2]],).get(1));
console.log(new Array(3,).length);

class A {
  x: number;
  constructor(x: number) { this.x = x; }
}
class B extends A {
  constructor() { super(1,); }
}
console.log(new A(2,).x, new B().x);

// Generic `new` takes the same tail.
class Box<T> {
  v: T;
  constructor(v: T) { this.v = v; }
}
console.log(new Box<number>(5,).v);

// A bare `new` with no parens is unaffected, and so are the other
// trailing-comma positions.
class Z { y: number = 1; }
console.log(new Z().y, new Z.prototype.constructor().y);
function f(a: number, b: number,): number { return a + b; }
console.log(f(1, 2,), [1, 2, 3,].length, { a: 1, }.a);

// Two commas in a row is still an error everywhere.
console.log(new Error("still one argument",).message);
