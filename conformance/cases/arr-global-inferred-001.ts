// Cluster-`values` follow-up — an un-annotated all-literal Array
// init on a top-level binding promotes to a real global slot (T[]
// spelling synthesized on both checker and lower sides), so named-fn
// bodies and parameter-default guards can read it. This is the
// dominant test262 prelude shape: `var values = [2, 1, 3];`.
var values = [2, 1, 3];
function f(x = values): number { return x[0] }
console.log(f())
console.log(f([9]))

function sum(): number {
  let t = 0;
  for (const n of values) t += n;
  return t;
}
console.log(sum())

// let form + mutation from a fn body
let names = ["ada", "grace"];
function firstName(): string { return names[0] }
function addName(): number { names.push("mary"); return names.length }
console.log(firstName())
console.log(addName())
console.log(names[2])

// const form, boolean elements
const flags = [true, false];
function flip(): boolean { return flags[1] }
console.log(flip())

// mixed int/fractional elements unify to the wide f64 slot
var mixed = [1, 2.5];
function widen(): number { return mixed[0] + mixed[1] }
console.log(widen())

// mixed SHAPES stay main-local (no promote) — top-level reads still work
var grab = [1, "x"];
console.log(grab.length)
