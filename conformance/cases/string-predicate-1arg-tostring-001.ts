// §22.1.3.{7,8,23} — the 1-arg needle of includes / startsWith / endsWith
// is the same IsRegExp-then-ToString slot as the 2-arg one.
const o = { toString() { return "b"; } };
console.log("abc".includes(o), "abc".startsWith(o), "abc".endsWith(o));
console.log("abc".includes(true as any), "a,b".includes([","] as any));
console.log("true".startsWith(true), "1".endsWith(1), "null".includes(null));
console.log("undefined".includes(undefined), "abc".includes([] as any));
const log: string[] = [];
const p = { toString() { log.push("ts"); return "c"; } };
console.log("abc".endsWith(p), log.join());
try { "abc".includes(Symbol() as any); } catch (e) { console.log((e as any).constructor.name); }
try { "abc".includes({ toString() { throw new RangeError("x"); } }); } catch (e) { console.log((e as any).constructor.name); }
