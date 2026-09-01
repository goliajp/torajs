// A `const t = <arrow>` binding taking a fn-typed parameter and calling
// it (rotation 549 — 549-02): the arrow is called by its binding name,
// so the by-callee-name AST rounds (closure-param retag, contextual
// callback typing) never reached the lifted decl — the body dispatched
// a closure cell as a raw code address (SIGBUS) while the same program
// spelled `function t(...)` worked.

// callback shapes into a fn-typed arrow param
const call = (f: () => number) => f();
console.log(call(() => 1));
let k = 10;
console.log(call(() => k));
console.log(call(() => k + 1), call(() => 5));
const g = () => 7;
console.log(call(g));
function h() { return 8; }
console.log(call(h));
console.log(call(function () { return 9; }));

// param shapes: statement body, annotated locals, explicit return type
const stmt = (f: () => number) => { const r = f(); return "v:" + r; };
console.log(stmt(() => 1));
const typed = (f: () => number): string => { const r: number = f(); return "v:" + r; };
console.log(typed(() => 2));
const strs = (f: () => string) => { const r = f(); return "v:" + r; };
console.log(strs(() => "x"));
const withArg = (f: (x: number) => number) => f(2);
console.log(withArg((x: number) => x + 1));
const two = (x: number, f: () => number) => x + f();
console.log(two(1, () => 2));
const voidCb = (f: () => void) => { f(); };
voidCb(() => { console.log("z"); });

// contextual return typing: `unknown` / `any` callbacks answering numbers
const unk = (f: () => unknown) => { const r = f(); return "v:" + r; };
console.log(unk(() => 1));
const anyRet = (f: () => any) => { const r = f(); return "v:" + r; };
console.log(anyRet(() => 1));
const unkExpr = (f: () => unknown) => f();
console.log(unkExpr(() => 1), unkExpr(() => "s"));

// the original repro shape: try/catch helper taking a thunk
const tag = (f: () => unknown): string => {
  try {
    const r = f();
    return "ok:" + JSON.stringify(r);
  } catch (e: any) {
    return "threw:" + e.constructor.name;
  }
};
console.log(tag(() => 1));
console.log(tag(() => { throw new TypeError("x"); }));
console.log(tag(() => ({ a: [1, 2] })));

// nested inside a function body
function outer() {
  const t = (f: () => number) => f();
  return t(() => 42);
}
console.log(outer());
