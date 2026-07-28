// RFC 20260729-fn-value-any V1b — an UN-annotated binding whose init
// is a constructor call is not a "typed ident receiver": its method
// calls ride the runtime any-method lane, so a named-fn argument
// wraps through its forwarder (the raw FnSig used to panic the
// whole program at box_to_any). Here the wrapped call reaches the
// runtime and the missing method throws catchably instead.
function foo() {}
let f = new foo();
function cb(): boolean {
  return true;
}
try {
  f.every(cb);
  console.log("no throw");
} catch (e: any) {
  console.log("caught", typeof e);
}
console.log("after");
