// RFC 20260810-indirect-argc-abi S2 — a closure called through a
// NARROWER Function face binds its unpassed params undefined
// (§10.2.1.4 argument binding via the hidden argc slot), so a
// defaulted param fires its default instead of reading a garbage
// register, and a plain any param answers undefined.
function t(cb: () => void) {
  cb();
}
t((p = 42) => {
  console.log("dflt", p);
});
function t2(cb: () => void) {
  cb();
}
t2((a: any) => {
  console.log("missing-is-undef", a === undefined);
});
// The three §10.2.1.4 states on a direct call: missing fires the
// default, explicit undefined fires it too, a passed value wins.
const direct = (p = 42) => p;
console.log(direct());
console.log(direct(undefined));
console.log(direct(7));
const two = (a: any, b = "x") => console.log(a, b);
two(1);
two(1, "y");
