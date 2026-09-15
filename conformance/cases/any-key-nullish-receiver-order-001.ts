// §6.2.5.5 GetValue — ToObject(base) throws before the deferred
// ToPropertyKey runs: `null[{toString}]` never calls toString. (The
// write form `null[k] = v` is spec'd the same way; bun runs toString
// first there, so it is not compared here. The compound form `u[k] +=
// 1` coerces its key in the lowering before the read — the registered
// compound-assignment single-evaluation follow-up.)
var log: string[] = [];
var k: any = { toString: function() { log.push("ts"); return "x"; } };
var n: any = null;
try { n[k]; } catch (e) { log.push((e as any).constructor.name); }
console.log("get", log.join(","));
log = [];
var o: any = { x: 1 };
console.log("object", o[k], log.join(","));
log = [];
try { n[0]; } catch (e) { log.push((e as any).constructor.name); }
try { n["y"]; } catch (e) { log.push((e as any).constructor.name); }
console.log("plain keys", log.join(","));
