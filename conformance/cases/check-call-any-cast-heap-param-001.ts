// r381 — an `as` cast over an `any` value erases nothing at runtime,
// so `f(z as P)` handed a NaN box to a reader using P's declared
// offsets: struct fields answered `0` / `null` and a fn-typed param
// answered SIGBUS, both silently past the checker's equality shortcut
// (the cast made the argument's type EQUAL the parameter's). The cast
// now routes to the same monomorph lane the uncast spelling takes, so
// the two spellings agree.

interface P { x: number; y: string }
const z: any = { x: 42, y: "s" };
function takeStruct(p: P) { console.log("struct", p.x, p.y); }
takeStruct(z as P);
takeStruct(z); // the uncast twin — same lane, same answer

const w: any = function () { return 7; };
function takeFn(f: () => number) { console.log("fn", f()); }
takeFn(w as () => number);

const y: any = [1, 2, 3];
function takeArr(xs: number[]) { console.log("arr", xs.length, xs[0], xs[1] + xs[2]); }
takeArr(y as number[]);

class C { v = 5; get() { return this.v; } }
const c: any = new C();
function takeInst(o: C) { console.log("inst", o.get(), o.v); }
takeInst(c as C);

const m: any = new Map<string, number>([["k", 9]]);
function takeMap(mm: Map<string, number>) { console.log("map", mm.get("k"), mm.size); }
takeMap(m as Map<string, number>);

// the cast does not buy past a real error: a non-callable behind `any`
// still throws where bun throws
const bad: any = 5;
try {
  takeFn(bad as () => number);
} catch (err) {
  console.log("caught:", (err as Error).constructor.name);
}

// a cast over a genuinely typed value is untouched — no widen, and the
// typed caller keeps reaching the original decl
const typed: number[] = [7, 8, 9];
takeArr(typed as number[]);

// mutation through the widened parameter is visible to the caller: the
// lane hands over the same block, it does not copy
function bump(xs: number[]) { xs[0] = xs[0] + 100; }
bump(y as number[]);
console.log("after", y[0], y[1], y[2]);

// nested: the cast argument is itself the result of a call
function makeAny(): any { return { x: 1, y: "n" }; }
takeStruct(makeAny() as P);
