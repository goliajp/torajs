// this-value-non-object probe (RFC date-invalid-time 刀 3 勘察)
const setDate = Date.prototype.setDate;
console.log(typeof setDate);
let called = 0;
const arg = { valueOf: () => { called += 1; return 1; } };
try { (setDate as any).call(0, arg); console.log("no throw number"); } catch (e) { console.log("throw", (e as Error).name); }
try { (setDate as any).call(null, arg); console.log("no throw null"); } catch (e) { console.log("throw", (e as Error).name); }
try { (setDate as any).call("", arg); console.log("no throw string"); } catch (e) { console.log("throw", (e as Error).name); }
console.log("called", called);
const getTime = Date.prototype.getTime;
try { (getTime as any).call(5); console.log("no throw getTime"); } catch (e) { console.log("throw", (e as Error).name); }
