// RFC 20260817-fnsig-reabstraction-thunk — a head-less FnDecl whose
// declared face disagrees with the fn-typed param slot it is passed
// into travels as a synthesized sig-exact thunk, so §10.2.1.4
// argument binding (defaults on unpassed positions) and the prefix
// repr widening both hold on the bare-FnSig call lane.
function mk(): number {
  console.log("default evaluated");
  return 42;
}
function g(p = mk()) {
  console.log("g sees:", p);
}
function callit(f: () => void) {
  f();
}
console.log("-- direct --");
g();
console.log("-- indirect zero-arity slot --");
callit(g);
console.log("-- any lane --");
const h: any = g;
h();
console.log("-- throwing default through the slot --");
function boom(p = (() => { throw new Error("from default"); })()) {}
try {
  callit(boom);
} catch (e) {
  console.log("caught:", (e as Error).message);
}
console.log("-- full-arity typed slot (prefix widening) --");
function call1(f: (p: number) => void) {
  f(7);
}
call1(g);
console.log("-- expression defaults, several spellings --");
function ga(p = 40 + 2) {
  console.log("ga sees:", p);
}
function gc(p: any = mk()) {
  console.log("gc sees:", p);
}
callit(ga);
callit(gc);
console.log("-- literal default keeps working --");
function gl(p = 5) {
  console.log("gl sees:", p);
}
callit(gl);
call1(gl);
