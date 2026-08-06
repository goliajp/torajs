// A class instance is a struct, and Object.values / Object.assign are
// the two surfaces that never said so — they matched the struct shape
// directly instead of resolving the class reference first, so the same
// object was a struct to Object.keys / entries / JSON and a ClassRef to
// these two.

class Point {
  x: number = 1;
  y: number = 2;
}

const p = new Point();
console.log(Object.keys(p).join(","));
console.log(Object.entries(p).map((e) => e[0] + "=" + e[1]).join(" "));
console.log(Object.values(p).join(","), Object.values(p).length);
console.log(JSON.stringify(Object.assign({ x: 0, y: 0 }, p)));

// Chained sources, left to right.
class Shift {
  x: number = 9;
  y: number = 8;
}
console.log(JSON.stringify(Object.assign({ x: 0, y: 0 }, p, new Shift())));

// The redefined-member gate applies to a class instance too.
const q = new Point();
Object.defineProperty(q as any, "x", { enumerable: false });
console.log(Object.values(q).join(","), Object.values(q).length);
console.log(Object.entries(q).map((e) => e[0]).join(","));
console.log(JSON.stringify(Object.assign({ x: -1, y: -1 }, q)));
console.log(q.x, JSON.stringify(q));

// A ctor-assigned field reads the same way.
class Named {
  a: number;
  b: number;
  constructor(a: number, b: number) {
    this.a = a;
    this.b = b;
  }
}
const n = new Named(3, 4);
console.log(Object.values(n).join(","), JSON.stringify(Object.assign({ a: 0, b: 0 }, n)));
