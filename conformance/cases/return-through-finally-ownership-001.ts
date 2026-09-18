// Returns routed through a `finally`: the frames outside the try are
// released at the finally tail, one site shared by every such return, so a
// returned local is retained rather than moved. Two returns in one try,
// a local of the try body, a return after the statement, nested finallys
// and a return from the catch all hand back the right value.
let fin = 0;
function two(i: number): string { const s = "s" + i; const t = "t" + i; try { if (i % 2 === 0) return s; return t; } finally { fin++; } }
function inner(i: number): string { try { const s = "in" + i; if (i % 2 === 0) return s; fin++; } finally { fin++; } return "out" + i; }
function after(i: number): string { const s = "s" + i; const o = "o" + i; try { if (i < 0) return s; } finally { fin++; } return o + s; }
function nested(i: number): any { const a: any = { who: "a" + i }; try { try { if (i % 3 === 0) return a; } finally { fin++; } return { who: "mid" + i }; } finally { fin++; } }
function caught(i: number): string { const s = "c" + i; try { if (i % 2 === 0) throw new Error("x"); return s; } catch (e) { return s + "!"; } finally { fin++; } }
let acc = 0; let text = "";
for (let i = 0; i < 3000; i++) {
  const a = two(i), b = inner(i), c = after(i), d: any = nested(i), e = caught(i);
  acc += a.length + b.length + c.length + d.who.length + e.length;
  if (i < 3) text += [a, b, c, d.who, e].join(",") + ";";
}
console.log(acc, fin, text);
