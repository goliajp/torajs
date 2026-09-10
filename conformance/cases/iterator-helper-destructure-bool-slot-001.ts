// A Bool slot taking an `any` value. `done` is typed by the typed
// iterator result it was destructured from; the later destructuring
// assignment feeds it the any-lane helper's result, and the typed
// lane refused the store outright ("slot is Bool but value is Any")
// where every other primitive slot already unboxes at the boundary.
//
// Recorded boundary (630-04): the unbox is ToBoolean, so a non-boolean
// `any` reaching a Bool slot stores its truthiness, not the value —
// the same posture the Str / Number arms have carried. Every shape
// below hands the slot a genuine boolean, where ToBoolean is identity.
function* upto(n: number): any {
  for (let i = 0; i < n; ++i) {
    yield i;
  }
}
const iterator = upto(5);
const dropped: any = (iterator as any).drop(2);
let { value, done } = (iterator as any).next();
console.log(value, done, typeof done);
({ value, done } = dropped.next());
console.log(value, done);
({ value, done } = dropped.next());
console.log(value, done);
({ value, done } = dropped.next());
console.log(value, done, typeof done);

// The plain-assignment face of the same rule, at top level and from
// inside a fn body.
let passed = false;
const o: any = { foo: true };
passed = o.foo;
console.log(passed, typeof passed);
function set(v: any): void {
  passed = v;
}
set(false);
console.log(passed, typeof passed);
