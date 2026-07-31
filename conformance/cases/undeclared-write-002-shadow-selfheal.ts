// undeclared-write mark self-heal on shadowed params (rotation 264
// regression fix): an IIFE param shadowing an OUTER fn param left a
// speculative-pass mark on the inner write target behind, and the
// assign lane threw a spurious "a is not defined" ReferenceError
// (test262 parameter-name-shadowing-parameter-name-let-const-and-var).
// The resolved-target self-heal now mirrors the read side.

// minimal form: outer param, same-name IIFE param, inner write.
// The params stay UNANNOTATED — the annotated form took a different
// checker path and never left the stale mark.
function f1(a) {
  (function (a) {
    a = 2;
    console.log("inner", a);
  })(1);
  console.log("outer", a);
}
f1(1);

// full test262 shape: param + let + var + const all shadowed,
// under-applied IIFE (c, d arrive undefined), writes inside
function f2(a) {
  let b = 1;
  var c = 1;
  const d = 1;
  (function (a, b, c, d) {
    a = 2;
    b = 2;
    c = 2;
    d = 2;
    console.log("inner", a, b, c, d);
  })(1, 1);
  console.log("outer", a, b, c, d);
}
f2(1);

// write through to a global still throws (the real undeclared-write
// lane must survive the self-heal)
try {
  // @ts-ignore
  notDeclaredAnywhere = 5;
} catch (e: any) {
  console.log("caught:", e instanceof ReferenceError);
}
