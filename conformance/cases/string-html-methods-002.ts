// Annex B B.2.2 HTML methods through an `any` receiver — the
// interned-id dispatch arm (ids 95-107) + name/length reflection
// off the String.prototype cells.

const s: any = "xy";
console.log(s.anchor("n"), s.link("http://a/b?q=1"));
console.log(s.fontcolor('re"d'), s.fontsize(7));
console.log(s.big(), s.blink(), s.bold(), s.fixed());
console.log(s.italics(), s.small(), s.strike(), s.sub(), s.sup());
// undefined attribute renders "undefined"
console.log(s.anchor(undefined));
// ShortStr immediate receiver materializes
const t: any = "k";
console.log(t.bold());
// Substr view receiver materializes in the glue
const u: any = ("ab" + "cd").slice(1, 3);
console.log(u.italics());
// UTF-16 receiver / attribute value widen the result
const w: any = "汉";
console.log(w.big(), s.anchor("宽"));
// chaining stays in the any world
console.log(s.bold().italics());
// name / length reflection off the prototype cells
console.log(String.prototype.anchor.name, String.prototype.anchor.length);
console.log(String.prototype.bold.name, String.prototype.bold.length);
console.log(String.prototype.fontsize.name, String.prototype.fontsize.length);
