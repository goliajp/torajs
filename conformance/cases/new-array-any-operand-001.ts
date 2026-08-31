const n: any = 3;
const f: any = 2.5;
const s: any = "3";
const u: any = undefined;
const b: any = true;
const o: any = { k: 1 };
const neg: any = -1;
const q: any = NaN;
const big: any = 4294967296;
const long: any = "a-string-too-long-for-shortstr-inline-encoding";
console.log(new Array(n).length);
console.log(JSON.stringify(new Array(s)));
console.log(new Array(u).length, new Array(u)[0]);
console.log(JSON.stringify(new Array(b)));
console.log(new Array(o).length, new Array(o)[0].k);
console.log(new Array(long)[0]);
try { new Array(f); } catch (e) { console.log("frac", e instanceof RangeError); }
try { new Array(neg); } catch (e) { console.log("neg", e instanceof RangeError); }
try { new Array(q); } catch (e) { console.log("nan", e instanceof RangeError); }
try { new Array(big); } catch (e) { console.log("big", e instanceof RangeError); }
