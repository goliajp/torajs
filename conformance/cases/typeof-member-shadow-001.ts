// rotation 154 — `typeof o.<protoMethod>` on an Any receiver answers
// the RUNTIME member read, not the static "function" shortcut, so an
// own shadow (undefined / non-callable) classifies truthfully.
// `constructor` keeps the shortcut (no value-read support yet).
const o: any = { a: 1 };
console.log(typeof o.toString, typeof o.toLocaleString, typeof o.constructor);
console.log(typeof o.hasOwnProperty, typeof o.valueOf);
const sh: any = { toString: undefined, toLocaleString: undefined, valueOf: undefined };
console.log(typeof sh.toString, typeof sh.toLocaleString, typeof sh.valueOf);
const nsh: any = { toString: 42 };
console.log(typeof nsh.toString);
function f0() { return 1; }
const f: any = f0;
console.log(typeof f.toString);
f.toString = undefined;
console.log(typeof f.toString);
