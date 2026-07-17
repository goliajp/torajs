// Uninit-let splice execution order (Date time-clip family root):
// `let r; <effects>; r = effectful();` used to splice the
// assignment's VALUE up into the declaration, running the setter
// BEFORE the intermediate statements — the getter observed
// post-mutation state. The declaration now moves DOWN to the
// assignment site: identical typing, effects in program order.

const date = new Date(8.64e15);
let returnValue: any;
const before: any = date.getTime();
console.log(before); // 8640000000000000 (setter has NOT run yet)
returnValue = date.setDate(28);
console.log(Number.isNaN(returnValue)); // true (time-clipped)
console.log(Number.isNaN(date.getTime())); // true

// general effect ordering
let r2: any;
const log: number[] = [];
log.push(1);
r2 = (() => { log.push(2); return 9; })();
console.log(log.join(","), r2); // 1,2 9

// adjacent assignment (the always-safe shape) still splices
let r3: any;
r3 = 5;
console.log(r3); // 5
console.log("done");
