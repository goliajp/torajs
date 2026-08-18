// sec 27.1.4 helpers through .call keep the strict receiver rules:
// step 1 TypeError on a non-object (no ToObject wrap), and a
// non-callable next surfaces as TypeError when stepping.
const P: any = Iterator.prototype;
for (const recv of [null, undefined, 0, "abc", true] as any[]) {
  try { P.every.call(recv, () => true); console.log("no throw", typeof recv); }
  catch (e) { console.log("caught", (e as any)?.constructor?.name, typeof recv); }
}
try { P.every.call({ next: 0 }, () => true); console.log("no throw next0"); }
catch (e) { console.log("caught", (e as any)?.constructor?.name, "next0"); }
// a real object receiver honoring the protocol works through .call
let k = 0;
const src = { next() { k += 1; return k <= 2 ? { done: false, value: k } : { done: true, value: undefined }; } };
console.log(P.map.call(src, (x: number) => x * 3).toArray().join(","));
console.log("survived");
