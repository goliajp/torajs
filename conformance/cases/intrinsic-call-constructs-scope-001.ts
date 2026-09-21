// §20.5.1.1 step 1 — `Error("m")` constructs, because the Error
// hierarchy is defined to treat a call and a `new` alike (§20.5.6.2
// for the NativeErrors, §20.5.7.1 for AggregateError). That is a
// property of THAT hierarchy, not of "being an injected class".
//
// tr read it off the injected-intrinsic flag, which had been the
// same set when the flag was written and stopped being it when
// `DisposableStack` / `AsyncDisposableStack` joined for a different
// question (§10.1.13, which prototype an object gets when
// `newTarget.prototype` is not an object). Those two are ordinary
// classes for THIS question: their own step 1 says a call with
// undefined NewTarget throws.
//
// Two questions had one flag, and the one whose set changed was not
// the one being read.
function attempt(f: any): string {
  try {
    return "ran:" + String(f());
  } catch (e: any) {
    return "threw:" + (e && e.constructor ? e.constructor.name : "?");
  }
}
// The Error hierarchy constructs on a plain call.
console.log("Error         ", attempt(() => (Error as any)("m").message));
console.log("TypeError     ", attempt(() => (TypeError as any)("m").name));
console.log("RangeError    ", attempt(() => (RangeError as any)("m").name));
console.log("SyntaxError   ", attempt(() => (SyntaxError as any)("m").name));
console.log("AggregateError", attempt(() => (AggregateError as any)([], "m").name));
console.log("is real Error ", attempt(() => (Error as any)("m") instanceof Error));
// The disposable stacks do not — their step 1 throws on undefined
// NewTarget, exactly like a user class.
console.log("DisposableStack", attempt(() => (DisposableStack as any)()));
console.log("AsyncDisposable", attempt(() => (AsyncDisposableStack as any)()));
// …and still construct normally with `new`.
console.log("new DStack     ", attempt(() => typeof new DisposableStack()));
console.log("new ADStack    ", attempt(() => typeof new AsyncDisposableStack()));
console.log("DStack usable  ", attempt(() => {
  const s: any = new DisposableStack();
  let ran = false;
  s.defer(() => { ran = true; });
  s.dispose();
  return String(ran);
}));
// A user class is refused either way.
class K {}
console.log("user class     ", attempt(() => (K as any)()));
console.log("new user class ", attempt(() => typeof new K()));
