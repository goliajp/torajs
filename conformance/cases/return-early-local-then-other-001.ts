// `if (c) return s; return other;` — the moved mark the first return puts
// on `s` belongs to that return alone. Left standing it silenced `s`'s
// drop on the path that returns `other` (one cell stranded per call).
function early(mode: number): any { const s: any = { who: "s", mode }; if (mode === 0) return s; return { who: "other", mode }; }
function branches(mode: number): any { const s: any = { who: "s" }; const t: any = { who: "t" }; if (mode === 0) { return s; } else if (mode === 1) { return t; } return { who: "u" }; }
function text(mode: number): string { const s: string = "s" + mode; if (mode === 0) return s; return "o" + mode; }
function loop(xs: any[]): any { const miss: any = { who: "miss" }; for (const x of xs) { if (x.hit) return x; } return miss; }
let out = "";
let acc = 0;
for (let i = 0; i < 3000; i++) {
  const e: any = early(i % 2); acc += e.who.length;
  const b: any = branches(i % 3); acc += b.who.length;
  acc += text(i % 2).length;
  const l: any = loop([{ hit: false }, { hit: i % 2 === 0, tag: i }]); acc += l.who === "miss" ? 1 : 2;
  if (i < 3) out += e.who + b.who + text(i) + " ";
}
console.log(acc, out);
