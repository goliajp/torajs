// V3-18 wedge — function parameter destructuring pattern per
// ES spec §14.1.3 (a BindingPattern is a valid FormalParameter):
//   function f([a, b]: number[])         — array form
//   function f({ x, y }: T)              — object form
// Pre-fix the param parser bailed at 'expected parameter
// name, got LBracket / LBrace' the moment a `[` or `{`
// appeared in param position. The hoist-into-temp workaround
// (`function f(p) { let { x, y } = p; ... }`) was always
// available but added noise to every call-site that wanted
// the binding-pattern style.
//
// Implementation: a destr pattern at the param position
// synthesizes a fresh hidden binding name (`__param_destr_<id>`)
// and accumulates per-element / per-field
//   `let bound = synth[i]`     (array form)
//   `let bound = synth.field`  (object form)
// into a vec that gets prepended to the parsed body just
// before emitting Stmt::FnDecl. The synthetic name carries
// the user's `: T` ann so check.rs can still type the param.
//
// Reserved-word fields go through keyword_property_name;
// rename target must still be a real ident (matches the
// existing parse_object_destructuring constraint). Currently
// only wired in parse_fn — class methods and arrow fns will
// follow as their own wedges if needed.
//
// MVP: array form is flat (no elision, no rest). Both
// extensions land later if the surface area justifies them.

// Array destructuring in fn param.
function pair([a, b]: number[]): number { return a * b }
console.log(pair([3, 4]))                      // 12
console.log(pair([10, 20]))                    // 200

function sum3([a, b, c]: number[]): number { return a + b + c }
console.log(sum3([1, 2, 3]))                   // 6
console.log(sum3([10, 20, 30]))                // 60

// Object destructuring in fn param.
function namify({ first, last }: { first: string, last: string }): string {
  return first + " " + last
}
console.log(namify({ first: "Alice", last: "Smith" }))
                                               // Alice Smith

// Object destructuring with rename.
function distance(
  { x: x1, y: y1 }: { x: number, y: number },
  { x: x2, y: y2 }: { x: number, y: number },
): number {
  let dx = x1 - x2
  let dy = y1 - y2
  return Math.sqrt(dx * dx + dy * dy)
}
console.log(distance({ x: 0, y: 0 }, { x: 3, y: 4 }))
                                               // 5

// Mixed: ident params and destr params at the same call site.
function mix(prefix: string, [a, b]: number[]): string {
  return prefix + ": " + a + " + " + b + " = " + (a + b)
}
console.log(mix("sum", [3, 4]))                // sum: 3 + 4 = 7

// Destr param inside a chain — receiver of `for-of pts.map(...)`
// type inference still flows through.
function scan(pairs: { k: string, v: number }[]): number {
  let total = 0
  for (let { v } of pairs) total += v
  return total
}
console.log(scan([
  { k: "a", v: 1 },
  { k: "b", v: 2 },
  { k: "c", v: 3 },
]))                                            // 6

// Single-field numeric obj destr (avoids the field-string
// transfer ownership rule that affects returning a string
// taken from an object field).
function getX({ x }: { x: number }): number { return x }
console.log(getX({ x: 42 }))                   // 42
