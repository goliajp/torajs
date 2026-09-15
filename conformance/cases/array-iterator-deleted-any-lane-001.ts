// §7.4.2 GetIterator — `delete Array.prototype[Symbol.iterator]` is
// observable: the array lane is reached THROUGH the prototype entry,
// so afterwards destructuring / for-of / spread refuse, while
// Array.from keeps its §23.1.2.1 step 3 array-like fallback.
const src: any = [1, 2, 3];
try { const [a, b] = src; console.log("dstr", a, b); } catch (e: any) { console.log("dstr", e.constructor.name); }
try { for (const v of src) { console.log("forof", v); break; } } catch (e: any) { console.log("forof", e.constructor.name); }
console.log("from", Array.from(src).join(","));
delete (Array.prototype as any)[Symbol.iterator];
try { const [a, b] = src; console.log("dstr2", a, b); } catch (e: any) { console.log("dstr2", e.constructor.name); }
try { for (const v of src) { console.log("forof2", v); break; } } catch (e: any) { console.log("forof2", e.constructor.name); }
console.log("from2", Array.from(src).join(","));
try { console.log("spread2", [...src].join(",")); } catch (e: any) { console.log("spread2", e.constructor.name); }
const s: any = "ab";
const out: string[] = [];
for (const v of s) out.push(v);
console.log("string still iterates", out.join(""));
