// rotation 638 — §14.7.5: a for-in head's source is an Expression, so the
// comma form evaluates every operand left to right and enumerates the last.
// oracle = bun.
let effects = 0;
const keys: string[] = [];
for (const k in (effects++, { a: 1, b: 2 })) { keys.push(k); }
console.log("decl", keys.join(","), effects);
let x: string = "";
const seen: string[] = [];
for (x in null, undefined, { key: 0 }) { seen.push(x); }
console.log("expr", seen.join(","), x);
const vs: string[] = [];
for (var v in effects++, effects++, { z: 9 }) { vs.push(v); }
console.log("var", vs.join(","), effects);
