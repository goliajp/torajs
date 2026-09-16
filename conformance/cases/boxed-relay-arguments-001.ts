// The boxed adapter of a value face enters its target with the true
// argument list: a void target reading `arguments`, a function parent
// reached through `super(...)`, and a construct face leading with the
// receiver and new.target.

var stored: any;
function Store() { stored = arguments; }
Store(1, 2, 3);
console.log(stored.length);
const S: any = Store;
S(1, 2, 3);
console.log(stored.length);
S.call({}, 1, 2, 3, 4);
console.log(stored.length, stored[3]);

function Base1(this: any) { stored = arguments; }
class Sub1 extends (Base1 as any) { constructor() { super(1, 2, 3); } }
new Sub1();
console.log(stored.length);
function Base2(this: any, a: number) { stored = arguments; }
class Sub2 extends (Base2 as any) { constructor() { super(5, 6, 7); } }
new Sub2();
console.log(stored.length, stored[2]);

var expectedNewTarget = function () {};
var thisValue: any, actualNewTarget: any;
function Parent(this: any) {
  thisValue = this;
  stored = arguments;
  actualNewTarget = new.target;
}
class Child extends (Parent as any) {
  constructor() {
    super(1, 2, 3);
  }
}
const instance = Reflect.construct(Child, [4, 5, 6], expectedNewTarget);
console.log(thisValue === instance, stored.length, stored[0], actualNewTarget === expectedNewTarget);
const P: any = Parent;
new P(8, 9);
console.log(stored.length, stored[1], actualNewTarget === Parent);
