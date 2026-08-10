// §19.2.1.3 EvalDeclarationInstantiation — a direct eval in default
// parameter position that var-declares `arguments` throws SyntaxError
// when the function is called; async form rejects; arrow form is
// legal (no own `arguments` binding).
function f(p = eval("var arguments")) {
  var arguments;
}
try {
  f();
  console.log("r1 no-throw");
} catch (e) {
  console.log("r1", (e as any).constructor.name);
}
function g(p = eval("var arguments = 'x'")) {}
try {
  g();
  console.log("r2 no-throw");
} catch (e) {
  console.log("r2", (e as any).constructor.name);
}
const k = (p = eval("var arguments = 'param'")) => p;
console.log("r3", k());
async function h(p = eval("var arguments")) {}
h().then(
  () => console.log("r4 resolved"),
  (e: any) => console.log("r4 rej", e.constructor.name),
);
