// r380 — the third receiver shape for a detached object-literal
// method: reached through a PARAMETER, where the literal is bound in
// the caller. 001 widens the binding the read sits on and 002 wraps a
// returned literal; here the read is `p.read` inside the callee while
// the literal is `const obj` outside it, so the read has to travel
// back to the argument binding before the widen can land.
//
// Only the binding moves. Annotating the parameter `: any` instead
// was measured to still SIGSEGV, so no signature is rewritten -- the
// structural parameter types below are the point.

function via(p: { n: number; read(): number }) { return p.read; }

const obj = { n: 5, read() { return this.n; } };
const t = via(obj);
try { t(); } catch (err) { console.log("bare:", (err as Error).constructor.name); }
console.log(t.call({ n: 11 }));

// two hops -- an argument can itself be another fn's parameter
function inner(q: { n: number; read(): number }) { return q.read; }
function outer(p: { n: number; read(): number }) { return inner(p); }
const t2 = outer(obj);
try { t2(); } catch (err) { console.log("hop2:", (err as Error).constructor.name); }
console.log(t2.call({ n: 13 }));

// calls through the same parameter stay right
function useIt(p: { n: number; read(): number }) { return p.read(); }
console.log(useIt(obj));
console.log(obj.read());

// a literal handed in inline has no binding to widen and answers anyway
function fromInline(p: { n: number; read(): number }) { return p.read; }
const t3 = fromInline({ n: 9, read() { return this.n; } });
console.log(t3.call({ n: 4 }));

// a parameter never read as a value keeps its nominal receiver
function plain(p: { v: number; show(): number }) { return p.show(); }
console.log(plain({ v: 2, show() { return this.v; } }));
