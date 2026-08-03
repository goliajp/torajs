// RFC 20260804-mutable-let-widen — a mutable unannotated `let`
// reassigned a value of a DIFFERENT syntactic family types `any`
// from declaration (JS semantics); same-family / unclassifiable
// reassigns stay on the typed lane.

// dominant cluster shape: New -> Iterator.from, then a helper call
class Counter {
  n: number = 0;
  next(): any {
    this.n = this.n + 1;
    return { done: this.n > 3, value: this.n };
  }
}
let c = new Counter();
c = Iterator.from(c);
const arr = c.toArray();
console.log(arr.length);

// null -> New
class D {
  d(): string {
    return "d";
  }
}
let maybe = null;
maybe = new D();
console.log(maybe.d());

// New(C) -> New(D)
class C2 {
  tag(): string {
    return "c";
  }
}
let x = new C2();
x = new D();
console.log(x.d());

// New -> NsCall
class W {
  n: number = 0;
}
let w = new W();
w = Object.create(null);
console.log(typeof w);

// Arr -> Obj
let z = [1, 2];
z = { a: 1 };
console.log(z.a);

// negatives: same-family / unclassifiable rhs stay typed
let i = 0;
i = i + 1;
i = 42;
console.log(i);
let s = "a";
s = "b";
console.log(s);
function mk(): string {
  return "z";
}
let t = "x";
t = mk();
console.log(t);
