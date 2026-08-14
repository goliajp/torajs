// A method stored on a FUNCTION value was invoked with the function's
// props bag as `this`, not the function. Property reads could not tell
// the difference -- the properties are in that bag -- so everything
// spelled `this.<name>` answered, and everything that wanted the
// function itself did not.
const K: any = function (this: any, p: number) {
  this.x = p;
};
K.tag = 7;

// Identity and `typeof`: the bag is an object, K is a function.
K.self = function (): any {
  return this;
};
console.log(K.self() === K, typeof K.self());

// A read off `this` -- this always worked, and must keep working.
K.readTag = function (): number {
  return (this as any).tag;
};
console.log(K.readTag());

// Calling a sibling through `this`.
K.t = function (): number {
  return 4;
};
K.s = function (): number {
  return (this as any).t() + 1;
};
console.log(K.s());

// `new this(...)` -- the bag is not a constructor, the function is.
K.make = function (p: number): any {
  return new (this as any)(p);
};
console.log(K.make(3).x);

// `this.prototype` reaches the function's prototype object.
K.prototype.z = 9;
K.proto = function (): any {
  return (this as any).prototype;
};
console.log(K.proto().z);

// A plain object receiver was never affected, and is unchanged.
const o: any = { tag: 1 };
o.self = function (): any {
  return this;
};
console.log(o.self() === o);
