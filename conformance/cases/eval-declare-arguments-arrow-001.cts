// declare-arguments arrow lane, legal half — §19.2.1.3: a direct eval
// in a true arrow's default-parameter position that var-declares
// `arguments` creates the binding in the arrow's parameter scope
// (fresh per call). Later defaults and the body resolve a bare
// `arguments` to it; a body that declares its own `arguments` shadows
// only body references (t262 arrow-fn-*-declare-arguments-assign).
var count = 0;

// no pre-existing binding: body read sees the eval's binding
var f1 = (p = eval("var arguments = 'param'")) => {
  console.log("f1:", arguments, p);
  count++;
};
f1();

// a later default's arrow closes over the same parameter-scope binding
var f2 = (p = eval("var arguments = 'param'"), q = () => arguments) => {
  console.log("f2:", q(), arguments === q());
  count++;
};
f2();

// body var-binding shadows body reads; the default arrow still sees
// the parameter-scope binding
var f3 = (p = eval("var arguments = 'p3'"), q = () => arguments) => {
  var arguments = "local";
  console.log("f3:", arguments, q());
  count++;
};
f3();

// second call gets a fresh parameter scope, not a stale cell
var f4 = (p = eval("var arguments = 'fresh'"), q = () => arguments) => q();
console.log("f4:", f4(), f4());

// a passed argument skips the default: eval never runs and nothing
// overwrites the argument (a bare read in that state resolves OUTSIDE
// the arrow — dynamic-declaration semantics the static rewrite does
// not model, so it stays unobserved here)
var f5 = (p = eval("var arguments = 'unused'")) => p;
console.log("f5:", f5(42));

console.log("count:", count);
