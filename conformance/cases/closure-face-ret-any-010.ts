// A function declared to return a scalar that hands back a value of any
// kind is called through fn-typed slots that each answer what it
// returns as is: a binding it initializes, a declared function's
// parameter, and a binding such a parameter initializes.
const raw: any = "5";
function h(): number {
  return raw;
}
const k: () => number = h;
function run(f: () => number): number {
  return f();
}
function viaLet(f: () => number): number {
  const j: () => number = f;
  return j();
}
function twice(f: () => number): number {
  return run(f);
}
console.log(k(), typeof k(), run(h), typeof run(k));
console.log(viaLet(h), typeof viaLet(k), twice(h), typeof twice(k));
