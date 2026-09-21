// §20.5.1.1 step 1 — `Error("m")` and `new Error("m")` do the same
// thing; the call form is not refused, it constructs. tr models that
// correctly at runtime: `call_class_ctor` sees an intrinsic class and
// sends it down the construct channel.
//
// What it could not do is GET there. Reaching a class through the
// construct channel needs a boxed factory adapter, and the compiler
// only synthesizes those for a program that "constructs from a
// value" — a predicate that enumerates the shapes reaching
// [[Construct]] through a `new`. The call position arrives through a
// plain call instead, so it was not in the list, and `Error("m")` in
// a program with no `new`-through-value anywhere answered "this
// constructor cannot be reached through a runtime value yet" about a
// constructor the program had named directly.
//
// Injected intrinsics are now admitted unconditionally. The set is
// the compile-time mirror of the runtime's own `class_is_intrinsic`
// test, so it is exactly the set that can arrive — a user class pays
// nothing, and is still refused by §10.2.1 step 2.
function attempt(f: any): string {
  try {
    return "ran:" + String(f());
  } catch (e: any) {
    return "threw:" + (e && e.constructor ? e.constructor.name : "?");
  }
}
console.log("Error(m)       ", attempt(() => (Error as any)("m").message));
console.log("TypeError(m)   ", attempt(() => (TypeError as any)("m").message));
console.log("RangeError(m)  ", attempt(() => (RangeError as any)("r").name));
console.log("through alias  ", attempt(() => {
  const E: any = Error;
  return E("m").message;
}));
console.log("through spread ", attempt(() => {
  const a: any = ["m"];
  return (Error as any)(...a).message;
}));
console.log("is a real Error", attempt(() => {
  const e: any = (Error as any)("m");
  return String(e instanceof Error) + "/" + e.name;
}));
console.log("no message     ", attempt(() => JSON.stringify((Error as any)().message)));
// §10.2.1 step 2 — a user class still refuses the call form.
class U {}
console.log("user class     ", attempt(() => (U as any)()));
// And the `new` forms are unmoved.
console.log("new Error      ", attempt(() => new Error("z").message));
console.log("new TypeError  ", attempt(() => new TypeError("t").name));
