// A call whose `any` argument fills a `string` parameter answers the
// value its body computes from that argument, not a string made from it.
function echo(s: string) { return s; }
const n: any = null;
const u: any = undefined;
const k: any = 42;
const str: any = "hi";
console.log(echo(n), typeof echo(n));
console.log(typeof echo(u), echo(u) === undefined);
console.log(typeof echo(k), echo(k));
console.log(echo(str), typeof echo(str), echo(str).length);
function echo2(s: string): string { return s; }
console.log(typeof echo2(n), typeof echo2(k), echo2(str));
try {
  console.log(echo(n).length);
} catch (e) {
  console.log(e instanceof TypeError);
}

// returns that do not hand the parameter back keep their type
function bang(s: string) { return s + "!"; }
console.log(bang(n), typeof bang(n));
function yes(s: string) { return true; }
console.log(yes(n), typeof yes(n));

// a method and a const arrow
class Box {
  tag(s: string) { return s; }
}
const b = new Box();
console.log(typeof b.tag(n), typeof b.tag(k), b.tag(str));
const arrow = (s: string) => s;
console.log(typeof arrow(n), typeof arrow(k));

// null / undefined answered by the typed body for a string argument
const one0: any = "z";
function maybe(s: string): string | null { return s.length > 1 ? s : null; }
console.log(maybe(str), maybe(one0), maybe("x"), typeof maybe(k));
function m2(s: string) { if (s.length > 1) return s; return undefined; }
const one: any = "a";
console.log(m2(str), m2(one), m2(one) === undefined, typeof m2(one));
const long: any = "abcdefghijklmnopqrstuvwxyz0123456789";
const parsed: any = JSON.parse('"a heap string from JSON.parse, long enough"');
function pick(s: string) { return s.length > 100 ? s : null; }
console.log(pick(long), typeof pick(long), pick(long) === null);
function pickU(s: string) { if (s.length > 100) return s; return undefined; }
console.log(pickU(parsed), typeof pickU(parsed), pickU(parsed) === undefined);
console.log(echo(long), typeof echo(long), echo(parsed).length);
const e2 = echo(parsed);
console.log(e2 === parsed, e2.toUpperCase().slice(0, 6));

// an `any[]` argument for a `number[]` parameter, declared `number` return
const seq = [];
seq.push("ab1");
function first(xs: number[]): number { return xs[0]; }
console.log(first(seq), typeof first(seq));

const x = echo(k);
console.log(x, typeof x);
let acc = 0;
const words: any[] = ["alpha-beta-gamma-delta", null, 7, "epsilon-zeta-eta-theta"];
for (const w of words) {
  const r = echo(w);
  acc += typeof r === "string" ? r.length : 1;
}
console.log(acc);
