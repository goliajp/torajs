// RFC 20260917-plain-fn-new-target knife 4 — super() into a plain
// function parent constructs it with the subclass's new.target.

var expectedNewTarget = function () {};
var thisValue: any, actualNewTarget: any;
function Parent(this: any) {
  thisValue = this;
  actualNewTarget = new.target;
}
class Child extends (Parent as any) {
  constructor() {
    super(1, 2, 3);
  }
}
const instance = Reflect.construct(Child, [4, 5, 6], expectedNewTarget);
console.log(thisValue === instance, actualNewTarget === expectedNewTarget);
const c = new Child();
console.log(actualNewTarget === Child, thisValue === c);

function P2(this: any, a: number) { this.a = a; this.nt = new.target; }
class Explicit extends (P2 as any) {
  constructor() { super(1); }
}
const e: any = new Explicit();
console.log(e.a, e.nt === Explicit, e instanceof Explicit, e instanceof P2);
class Implicit extends (P2 as any) {}
const i: any = new Implicit(2);
console.log(i.a, i.nt === Implicit);
