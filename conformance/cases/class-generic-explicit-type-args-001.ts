// `new Box<number>()` states what `T` is. The rewrite to the
// synthesized factory turns it into a plain call, and a call has
// nowhere to carry type arguments, so they were dropped — leaving
// argument-driven inference with nothing to go on whenever the type
// parameter appears only in the field types:
//
//     class Box<T> { items: T[] = [] }
//     const b = new Box<number>();
//     // could not infer type parameter `T` for `__new_Box`

class Box<T> {
  items: T[] = [];
}

const bn = new Box<number>();
bn.items.push(4);
console.log("number-box", bn.items[0], bn.items.length);

const bs = new Box<string>();
bs.items.push("x");
console.log("string-box", bs.items[0], bs.items.length);

const annotated: Box<number> = new Box<number>();
annotated.items.push(7);
console.log("annotated-binding", annotated.items[0]);

class Pair<K, V> {
  ks: K[] = [];
  vs: V[] = [];
}
const p = new Pair<string, number>();
p.ks.push("a");
p.vs.push(1);
console.log("two-params", p.ks[0], p.vs[0]);

// A callback on the field sees the stated element type.
const bm = new Box<number>();
bm.items.push(3);
console.log("field-callback", bm.items.map((x) => x * 2)[0]);

// The shapes that already worked, because the constructor argument
// pinned the parameter: with the type arguments written and without.
class Cell<T> {
  v: T;
  constructor(v: T) {
    this.v = v;
  }
}
console.log("ctor-arg-explicit", new Cell<number>(4).v);
console.log("ctor-arg-inferred", new Cell(5).v);
console.log("ctor-arg-string", new Cell<string>("s").v);

function id<T>(x: T): T {
  return x;
}
console.log("generic-fn", id(4), id("s"));

// A non-generic class is untouched.
class Plain {
  items: number[] = [];
}
const pl = new Plain();
pl.items.push(9);
console.log("non-generic", pl.items[0]);
