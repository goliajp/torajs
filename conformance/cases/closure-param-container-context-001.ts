// An anonymous arrow written straight into an annotated container
// carries that container's declared type inward, the way TypeScript
// types it. Only a directly annotated binding was doing so, and a
// parameter with no context falls back to `string` — so a field
// declared `(a: number) => number` read its argument as a Str
// pointer, and `typeof a` answered "string" inside it.

// Array element position, in every shape that reads one back.
const fs: ((n: number) => number)[] = [(n) => n + 1];
console.log("elem-call", fs[0](1));
const g = fs[0];
console.log("elem-via-let", g(1));

const fs2: ((n: number) => number)[] = [(n) => n + 1, (n) => n * 2];
console.log("elem-second", fs2[1](3));

const fs3: ((n: number) => number)[] = [(n) => n + 1];
for (const f of fs3) console.log("elem-for-of", f(5));

const cap = 10;
const fs4: ((n: number) => number)[] = [(n) => n + cap];
console.log("elem-capture", fs4[0](1));

// Object-literal field position: inline object type and a named one.
const o: { f: (n: number) => number } = { f: (n) => n + 1 };
console.log("field-inline-ann", o.f(1));

type Ops = { f: (n: number) => number };
const o2: Ops = { f: (n) => n * 3 };
console.log("field-typedecl", o2.f(4));

const o3: { f: (a: number, b: number) => number } = { f: (a, b) => a * 100 + b };
console.log("field-two-params", o3.f(3, 7));

const o4: { f: (a: boolean) => boolean } = { f: (a) => !a };
console.log("field-boolean", o4.f(true));

const o5: { f: (a: string) => string } = { f: (a) => a + "!" };
console.log("field-string", o5.f("hi"));

const o6: { f: (a: number) => number } = {
  f: (a) => {
    console.log("  inside typeof", typeof a, "raw", a);
    return a + 1;
  },
};
console.log("field-body", o6.f(3));

// Nested one level further: a field whose type is another object type
// carries its own fields' types inward too.
type Inner = { g: (a: number) => number };
const o7: { i: Inner } = { i: { g: (a) => a + 1 } };
console.log("nested-field", o7.i.g(1));

// Shapes that already worked and must keep working: a named function,
// an arrow bound to a name first, a method shorthand, an `any[]`
// container, an un-annotated container, a directly annotated binding,
// and a call-argument position.
function nm(n: number): number {
  return n + 1;
}
const fs5: ((n: number) => number)[] = [nm];
console.log("elem-named-fn", fs5[0](1));

const bound = (n: number): number => n + 1;
const o8: { f: (n: number) => number } = { f: bound };
console.log("field-bound-arrow", o8.f(1));

const o9 = { f(n: number) { return n + 1; } };
console.log("field-method-shorthand", o9.f(1));

const o10 = { f: (n: number) => n + 1 };
console.log("field-inferred-container", o10.f(1));

const fs6: any[] = [(n: number) => n + 1];
console.log("elem-any-container", fs6[0](1));

const fs7 = [(n: number) => n + 1];
console.log("elem-unann-container", fs7[0](1));

const direct: (n: number) => number = (n) => n + 1;
console.log("direct-binding", direct(3));

function take(cb: (n: number) => number): number {
  return cb(3);
}
console.log("call-arg", take((n) => n + 1));

// Containers with no function in them are untouched.
const plain: { a: number; s: string } = { a: 1, s: "x" };
console.log("plain-obj", plain.a, plain.s);
const nums: number[] = [1, 2, 3];
console.log("plain-arr", nums[1], nums.length);
