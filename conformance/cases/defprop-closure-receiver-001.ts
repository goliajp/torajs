// defineProperty over a function receiver (15.2.3.6-4-33 shape,
// T-27 Function-as-Object). Pre-fix define_apply walked the
// closure layout as a dynobj header — SIGSEGV on the first define
// against an any-typed function.

const fun: any = function () {};
Object.defineProperty(fun, "foo", { value: 12, configurable: false });
console.log(fun.foo); // 12

// non-configurable redefine rejects (§10.1.6.3)
try {
  Object.defineProperty(fun, "foo", { value: 11, configurable: true });
  console.log("no throw");
} catch (e) {
  console.log("caught:", e instanceof TypeError);
}
console.log(fun.foo); // 12 (unchanged)

// enumerable expando shows in the descriptor
const d: any = Object.getOwnPropertyDescriptor(fun, "foo");
console.log(d.value, d.configurable); // 12 false

// plain assignment expandos coexist
fun.bar = 5;
console.log(fun.bar); // 5
console.log("done");
