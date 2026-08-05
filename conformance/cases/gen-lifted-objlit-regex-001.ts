// A generator local holding an object literal whose fields the lift
// can read, and one holding a regex literal.
//
// Same composition gap as the array case: the shared sniff has an
// object-literal arm, but it recurses into its own arms only, so the
// whole literal became unreadable the moment one field was a shape
// only the lift knows. An arrow is the field that matters — the
// shared arm reads a signature published under a lifted `__closure_*`
// name, and this pass runs before those exist. So
//
//   const t = { n: 1, f: (x: number) => x * 2 };
//
// took the `number` fallback and `t.f(t.n)` after it reported "no
// member `.f` on type Number".
//
// A regex literal had no arm anywhere: `const r = /a(b)c/` fell back
// the same way, and every use of it went down with it ("no member
// `.exec` on type Number"). Its two methods answer here too, since
// the shared sniff's method table is keyed on string / T[] receivers
// and has nothing to say about a regex one.

// an object literal with a fn-valued field
function* withFn(): any {
  const t = { n: 3, f: (x: number) => x * 2 };
  yield t.f(t.n);
  yield t.n;
}
const wf = withFn();
console.log(wf.next().value);
console.log(wf.next().value);

// nested: an object literal holding a class instance and an array
class Box {
  v: number = 5;
}
function* nested(): any {
  const t = { b: new Box(), xs: [1, 2, 3] };
  yield t.b.v + t.xs[2];
}
console.log(nested().next().value);

// an array of object literals — both repeated arms, one inside the other
function* arrayOfObjs(): any {
  const rows = [{ k: 1 }, { k: 2 }];
  yield rows[0].k + rows[1].k;
}
console.log(arrayOfObjs().next().value);

// a regex literal and what its methods answer
function* rx(): any {
  const r = /a(b)c/;
  const m = r.exec("abc");
  yield m ? m[1] : "none";
  const hit = r.test("xabcx");
  yield hit;
}
const it = rx();
console.log(it.next().value);
console.log(it.next().value);

// a regex literal with flags, held across a yield
//
// Only `test` here, not `String.replace(r, s)`: handing a lifted
// regex FIELD to replace reaches a separate lowering gap ("console
// multi-arg coercion of type RegExp not supported"). That shape used
// to fail earlier, on the field's type; it still fails, one step
// later and just as loudly.
function* flags(): any {
  const g = /o/g;
  yield "before";
  yield g.test("foo");
}
const fl = flags();
console.log(fl.next().value);
console.log(fl.next().value);

// an all-number object literal still reads the way it always did
function* plain(): any {
  const p = { a: 1, b: 2 };
  yield p.a + p.b;
}
console.log(plain().next().value);
