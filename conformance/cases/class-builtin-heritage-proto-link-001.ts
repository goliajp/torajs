// §15.7.14 — a subclass's prototype object has the parent's prototype
// as its [[Prototype]], and a stripped builtin parent is no exception:
// the link is what makes an inherited method reachable by lookup
// rather than only by the runtime's own dispatch.
class MySet extends Set<number> {}
class MyMap extends Map<string, number> {}
class MyArr extends Array<number> {}
class MyDate extends Date {}
class MyRe extends RegExp {}
class MyPromise extends Promise<number> {}

console.log(Object.getPrototypeOf(MySet.prototype) === Set.prototype);
console.log(Object.getPrototypeOf(MyMap.prototype) === Map.prototype);
console.log(Object.getPrototypeOf(MyArr.prototype) === Array.prototype);
console.log(Object.getPrototypeOf(MyDate.prototype) === Date.prototype);
console.log(Object.getPrototypeOf(MyRe.prototype) === RegExp.prototype);
console.log(Object.getPrototypeOf(MyPromise.prototype) === Promise.prototype);

// A grandchild reaches the builtin prototype two links up.
class Deeper extends MySet {}
console.log(Object.getPrototypeOf(Deeper.prototype) === MySet.prototype);
console.log(Object.getPrototypeOf(Object.getPrototypeOf(Deeper.prototype)) === Set.prototype);

// The link is a lookup path, not a replacement for dispatch: an
// instance still answers from its own class first.
class Loud extends Set<number> {
  has(_x: number): boolean { return true; }
}
const l = new Loud();
console.log(l.has(1), new MySet().has(1));

// An own method on the subclass prototype shadows the builtin one.
console.log(typeof (MySet.prototype as any).add);
