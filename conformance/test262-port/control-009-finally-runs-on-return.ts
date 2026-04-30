// Adapted from test262: language/statements/return/* — finally runs even
// when a try-body returns. Verified by ordering: f() prints "before"
// from finally, then check() prints the return value.
function f(): number {
  try {
    return 1;
  } finally {
    console.log("before-return");
  }
}
console.log(f());
