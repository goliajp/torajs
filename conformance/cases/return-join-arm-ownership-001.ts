// A join in return position (`?:`, `??`, `||`, `&&`) hands back ONE arm,
// decided at run time: both arms' locals keep their scope drops and the
// answer takes its own stake. Marking both arms moved stranded the one
// not returned; the values below also prove nothing is released twice.
function tern(o: any, pickFirst: boolean): any { const a: any = { who: "a", n: o.k }; const b: any = { who: "b", n: o.k + 1 }; return pickFirst ? a : b; }
function nul(o: any): any { const a: any = { who: "fallback" }; const r: any = o; return r ?? a; }
function lor(o: any): any { const a: any = { who: "fallback" }; const r: any = o; return r || a; }
function land(o: any): any { const a: any = { who: "second" }; const r: any = o; return r && a; }
function fresh(): any { return { who: "fresh" }; }
function mixed(o: any): any { const a: any = { who: "local" }; return o === null ? fresh() : a; }
function strs(n: number): string { const a: string = "x" + n; const b: string = "y" + n; return n % 2 === 0 ? a : b; }
function Ctor(this: any, o: any): any { this.own = 1; return o; }
const K: any = { k: 1 };
let acc = 0;
let text = "";
for (let i = 0; i < 2000; i++) {
  const t: any = tern(K, i % 2 === 0); acc += t.n;
  acc += nul(K).k + nul(null).who.length + nul(undefined).who.length;
  acc += lor(K).k + lor(0).who.length;
  acc += land(K).who.length + (land(0) === 0 ? 1 : 0);
  acc += mixed(null).who.length + mixed(K).who.length;
  if (i < 4) text += strs(i);
  const made: any = new (Ctor as any)(i % 2 === 0 ? K : 5);
  acc += made === K ? 1 : made.own;
}
console.log(acc, text, K.k);
