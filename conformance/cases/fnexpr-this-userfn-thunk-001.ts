// rotation 460 — the no-receiver slot proved by READING THE CALLEE:
// a this-reading fn-expr handed to a program-local FnDecl whose
// parameter is only ever CALLED gets the receiver-less binding
// (§13.3.6.1 passes no receiver for a callee that is not a
// Reference; §10.2.1.2 step 5 keeps it undefined under the strict
// module goal). The `() => void` slot is the point: a concrete
// function signature is exactly what the promote knives must decline,
// so before this the program had no answer at all.
function run(t: () => void): void {
  t();
}
function runTwice(label: string, t: () => void): void {
  t();
  t();
}
function maybe(t: () => void, go: boolean): void {
  if (go) {
    t();
  }
}
let seen: string[] = [];
run(function () {
  seen.push(typeof this);
});
runTwice("x", function () {
  seen.push(this === undefined ? "undef" : "leak");
});
maybe(function () {
  seen.push("called");
}, true);
maybe(function () {
  seen.push("not-called");
}, false);
console.log(seen.join(","));

// The refutation side: a parameter the callee invokes through
// `.call` picks its own receiver, so this slot is NOT admitted — the
// explicit-`any` promote lane answers it instead.
function viaCall(f: any, recv: any): void {
  f.call(recv);
}
let tag: any = 0;
viaCall(function () {
  tag = (this as any).tag;
}, { tag: 41 });
console.log(tag);
