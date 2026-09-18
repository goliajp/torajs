// An index read that already answers an owned value (a keyed read through
// an `any` key) is returned as it is: no second retain, and the array it
// was read from keeps its scope drop.
function byAny(i: number): any { const xs: any[] = [{ n: i }, "s" + i, [i]]; const k: any = i % 3; return xs[k]; }
function byNum(i: number): any { const xs: any[] = [{ n: i }, "s" + i]; return xs[i % 2]; }
function byStr(i: number): any { const xs: any[] = [{ n: i }, "s" + i]; const k: any = "" + (i % 2); return xs[k]; }
function miss(i: number): any { const xs: any[] = [i]; const k: any = 7; return xs[k]; }
let acc = 0; let text = "";
for (let i = 0; i < 3000; i++) {
  const a: any = byAny(i), b: any = byNum(i), c: any = byStr(i), d: any = miss(i);
  acc += (typeof a === "string" ? a.length : 1) + (typeof b === "string" ? b.length : 2) + (typeof c === "string" ? c.length : 3) + (d === undefined ? 1 : 0);
  if (i < 3) text += JSON.stringify([a, b, c]) + (d === undefined ? "u" : "?") + ";";
}
console.log(acc, text);
