const TA: any = Object.getPrototypeOf(Int8Array);
const P: any = TA.prototype;
const bad: any[] = [null, undefined, true, "abc", 12, Symbol(), [1, 2, 3], { 0: 1, length: 1 }, Uint8Array.prototype, new DataView(new ArrayBuffer(4))];
for (const m of ["with", "toReversed", "toSorted", "at", "map", "join", "values", "fill"]) {
  let n = 0;
  for (const v of bad) { try { P[m].call(v, 0, 0); } catch (e) { if (e instanceof TypeError) n++; } }
  console.log(m, n);
}
const u8 = new Uint8Array([104, 105]);
console.log((u8 as any).toHex === (Uint8Array.prototype as any).toHex, (Uint8Array.prototype as any).toHex.call(u8));
try { (Uint8Array.prototype as any).toHex.call([1]); } catch (e) { console.log("toHex arr", (e as any).constructor.name); }
console.log(P.with.call(new Int8Array([1, 2]), 0, 9).join(","), P.at.call(new Float32Array([1.5]), 0));
