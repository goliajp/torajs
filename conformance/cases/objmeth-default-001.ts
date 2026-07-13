// RFC 20260714-t262-top-clusters 刀 1 — method-position param
// defaults. Un-annotated defaulted params in method positions had no
// type source (fn/arrow tiers ride implicit-generic TypeVar + call-
// site mono; methods dispatch without an instantiation site): the
// checker rejected class methods, obj-literal methods read garbage
// (SIGSEGV when the body used the param). Two fixes compose here:
// the param's ann is inferred from the default literal (TS-spec
// posture), and obj-literal method fields join the Member-callee
// default-padding table alongside the `__cm_` class grouping. The
// call-boundary arg coercion now also covers the struct-field
// closure dispatch (an Array-literal arg into an any param used to
// pass its typed repr raw).

// plain scalar default — obj-literal method
var o1 = { m(x = 5) { console.log("A:", x); } };
o1.m();
o1.m(9);

// defaulted tail param after a required one
var o2 = { n(a: number, b = 3) { console.log("B:", a, b); } };
o2.n(1);
o2.n(1, 4);

// ary-pattern destr param with whole-pattern default
var o3 = { mm([x, y = 7] = [1]) { console.log("C:", x, y); } };
o3.mm();
o3.mm([2, 3]);

// single-elem ary pattern
var o4 = { mq([x] = [5]) { console.log("D:", x); } };
o4.mq();

// obj-pattern with non-empty whole-pattern default
var o5 = { mo({ p = 2 } = { p: 9 }) { console.log("F:", p); } };
o5.mo();
o5.mo({ p: 11 });

// class method plain default (checker used to reject the decl)
class K {
  kk(x = 5) {
    console.log("E:", x);
  }
}
new K().kk();
new K().kk(9);

// explicit Array-literal arg into a destr param (no default in play)
var o6 = { mm2([x, y = 7]) { console.log("G:", x, y); } };
o6.mm2([1]);

// string / bool default literals infer their anns
var o7 = { s(t = "hi", f = true) { console.log("H:", t, f); } };
o7.s();

// any-held arrow regression guard (boxed-adapter lane still pads)
const af: any = (x = 6) => { console.log("I:", x); };
af();

console.log("done");
