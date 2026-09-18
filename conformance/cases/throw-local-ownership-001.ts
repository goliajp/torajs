// A throw never moves a local into the throw slot: the slot takes its own
// stake and every binding keeps its drop. The shapes below each read the
// thrown value AND the source binding afterwards, on both the path that
// threw and the path that did not.
function mkAny(i: number): any { return new Error("any" + i); }
function conditional(i: number): number { const e = new Error("bad " + i); if (i < 0) throw e; return e.message.length; }
function inLoop(n: number): number { let c = 0; for (let j = 0; j < 4; j++) { const e = new Error("x" + j + n); if (n < 0) throw e; c += e.message.length; } return c; }
function outerFrame(i: number): string { const e = new Error("outer" + i); let seen = ""; try { if (i % 2 === 0) throw e; seen = "none"; } catch (x) { seen = (x as Error).message; } return seen + "/" + e.message; }
function anyLocal(i: number): string { const e: any = new Error("anyl" + i); try { if (i >= 0) throw e; } catch (x) { return (x as any).message + "=" + e.message; } return "no"; }
function anyTemp(i: number): string { try { throw mkAny(i); } catch (x) { return (x as any).message; } return "no"; }
function rethrow(i: number): string { try { try { throw new Error("re" + i); } catch (e) { throw e; } } catch (o) { return (o as Error).message; } return "no"; }
class Holder { err: Error; constructor(i: number) { this.err = new Error("held" + i); } fire(): string { try { throw this.err; } catch (x) { return (x as Error).message; } return "no"; } }
let acc = 0;
let text = "";
for (let i = 0; i < 2000; i++) {
  acc += conditional(i) + inLoop(i);
  const o = outerFrame(i), a = anyLocal(i), t = anyTemp(i), r = rethrow(i);
  const h = new Holder(i); const f = h.fire() + h.fire() + h.err.message;
  acc += o.length + a.length + t.length + r.length + f.length;
  if (i < 2) text += [o, a, t, r, f].join(",") + ";";
}
try { conditional(-1); } catch (e) { text += (e as Error).message; }
try { inLoop(-1); } catch (e) { text += "|" + (e as Error).message; }
console.log(acc, text);
