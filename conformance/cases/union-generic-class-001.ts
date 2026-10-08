// A union of instantiations of one generic class: its methods and
// fields are members of each instantiation (TS §4.13).

class Box<T> {
  constructor(public v: T) {}
  get(): T {
    return this.v;
  }
  tag(): string {
    return "box";
  }
}

class Pair<A, B> {
  constructor(public a: A, public b: B) {}
  tag(): string {
    return "pair";
  }
}

function show(b: Box<number> | Box<string>): string {
  return b.tag() + ":" + b.get() + ":" + b.v;
}

function tagOf(x: Box<boolean> | Pair<number, string>): string {
  return x.tag();
}

console.log(show(new Box<number>(1)), show(new Box<string>("s")));
console.log(tagOf(new Box<boolean>(true)), tagOf(new Pair<number, string>(1, "b")));
