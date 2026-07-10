// Chunk 806 — a void call USED AS A VALUE is `undefined` (ES
// §14.10.1). The checker types user-fn void calls Undefined
// (general_call), every call lane answers the ConstPtrNull payload,
// and the Undefined-aware consumers (console print / strict-eq /
// typeof / template / concat / let-init / return box) see a real
// undefined. Pre-fix: the direct lane printed x0 residue garbage,
// closure lanes printed 0, typeof answered "object", `return
// voidCall()` in an inferred-any fn printed null.

function v(m: string) { console.log(m) }

// direct call in console.log
console.log(v("a"));

// strict-eq against undefined
console.log(v("b") === undefined);

// typeof still evaluates the operand for effect
console.log(typeof v("c"));

// let-init binds undefined after effect
const x = v("d");
console.log(x);

// return of a void call from an inferred-any fn
function g() { return v("e") }
console.log(g());

// plain `return undefined` from an inferred-any fn (same box lane)
function h() { return undefined }
console.log(h());

// template + string concat coerce to "undefined"
console.log(`r=${v("f")}`);
console.log("" + v("g"));

// closure lanes: annotated void global + local arrow
function logIt(m: string) { console.log(m) }
const f1: (m: string) => void = logIt;
console.log(f1("h"));
const f2 = (m: string): void => { console.log(m) };
console.log(f2("i"));

// class method void call
class C { m(s: string) { console.log(s) } }
const c = new C();
console.log(c.m("j") === undefined);
