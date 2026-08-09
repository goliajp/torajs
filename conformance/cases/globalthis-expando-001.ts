// RFC 20260807-global-object G2.5 — expando property mutation through
// bare `globalThis` lands on the runtime singleton: store / read-back /
// update / delete / string-literal index key. Builtin-name overrides
// (`globalThis.Array = x`) stay a compile reject — bare-name reads
// keep static resolution and would diverge silently.
globalThis.evaluations = [];
globalThis.evaluations.push(1);
globalThis.evaluations.push(2);
console.log(globalThis.evaluations.length);
globalThis.counter = 5;
globalThis.counter++;
console.log(globalThis.counter);
console.log(globalThis.notThere === undefined ? "miss-undef" : "bad");
delete globalThis.counter;
console.log(globalThis.counter === undefined ? "deleted" : "still");
globalThis["strKey"] = "sv";
console.log(globalThis["strKey"]);
delete globalThis["strKey"];
console.log(globalThis.strKey === undefined ? "key-deleted" : "still");
