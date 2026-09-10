// A function expression written INSIDE an eval text carries a span
// into that text, and `intern_fn_source` slices the program's own
// source with it — the statement half of this has been blanked since
// the foreign-span knife, the expression half had not. In a file
// whose byte 33 sits inside a multi-byte character (the test262
// harness's own header carries an em dash there) the slice panicked
// the compiler outright.
//
// Blanking the eval parse's expression spans is what the module
// resolver already does for a lib slice. Every line below must agree
// with bun; the `toString` reads are here so a future span change
// cannot go unnoticed.
var executed = false;
class C {
  x = eval("executed = true; () => 1;");
}
const c = new C();
console.log(executed, (c as any).x());
const g: any = eval("(function inner() { return 2; })");
console.log(g(), g.toString().indexOf("[native code]") >= 0);
const h: any = eval("(() => 3)");
console.log(h(), h.toString().indexOf("[native code]") >= 0);
