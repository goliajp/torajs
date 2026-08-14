// A class factory — the class value RETURNED out of the function that
// declares it. `return C` never calls the binding, but the cell escapes,
// so admitting it needs the any-lane proof rather than the "no call
// here" one: the binding is annotated `any` and the boundary does not
// re-type it, after which every call path honours the receiver channel.
// Before that, one `return C` took the whole binding off the promotion
// lane and the constructor's `this` stayed a capture nobody binds.
//
// The identity half is the point of the shape: each call of `make`
// mints a FRESH class closed over that call's environment, which is
// exactly what tr's static class machinery cannot model and why this
// lane exists.
function make(base: number) {
  class Counter {
    n: number;
    constructor(start: number) {
      this.n = start + base;
    }
    bump() {
      this.n = this.n + base;
      return this.n;
    }
    total() {
      return this.n + base;
    }
  }
  return Counter;
}

const Tens: any = make(10);
const Hundreds: any = make(100);

const a: any = new Tens(1);
const b: any = new Hundreds(1);

console.log(a.n, b.n);
console.log(a.bump(), b.bump());
console.log(a.total(), b.total());

// Two calls of the factory are two different classes, and an instance
// belongs to the one that made it.
console.log(Tens === Hundreds, a instanceof Tens, a instanceof Hundreds);

// The escaped value stays callable as a member of something else, and
// the receiver still arrives.
const holder: any = { Make: Tens };
console.log(new holder.Make(5).total());

// Returning it through a second hop, then constructing.
function relay(k: any): any {
  return k;
}
console.log(new (relay(Tens))(7).n);
