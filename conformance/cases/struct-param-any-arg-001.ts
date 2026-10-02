// §13.3.2.1 / §10.2.1.3 — a struct-typed parameter reached through
// `any` with an argument that is not a struct of that layout. The
// argument binds as it is and every `q.x` is a [[Get]] on that value:
// an array, a string, a function and a number answer undefined, null
// and undefined throw. The typed body still serves the fitting call.
type P = { x: number; y: number };
function f(p: P) { const q = p; return q.x; }
const g: any = f;
const shapes: [string, any][] = [
  ["array", [1, 2, 3]],
  ["string", "abc"],
  ["function", function () {}],
  ["number", 3],
];
for (const [name, v] of shapes) {
  try { console.log(name, "->", g(v)); } catch (e) { console.log(name, "throws", e instanceof TypeError); }
}
try { console.log("null ->", g(null)); } catch (e) { console.log("null throws", e instanceof TypeError); }
try { console.log("undefined ->", g(undefined)); } catch (e) { console.log("undefined throws", e instanceof TypeError); }
function r(p: P, ...rest: number[]) { return p.x; }
const gr: any = r;
console.log("rest ->", gr([1, 2], 3));
console.log("typed", f({ x: 7, y: 8 }), r({ x: 1, y: 2 }, 3));
