// RFC 20260821 attack B — a match result whose exec-shape properties
// (§22.2.7.8 index / input / groups) provably have no reader is built
// without them. The analysis is `ast/regex_result_props.rs`; this
// fixture pins BOTH faces, and the preserved face matters more: a
// false positive there is a silently missing property.
//
// Note the typed tier cannot spell `m.index` on an `Array(String)` at
// all (it is a type error) — every reader of the exec shape reaches it
// either through the print face or through an `any` slot, which is why
// those are the shapes the preserved half tests.

// --- optimized face: only element / length / null-compare reads ---
const m1: string[] | null = "xabby".match(/ab*/);
if (m1 !== null) console.log(m1[0], m1.length);

let m2: string[] | null = "zabq".match(/a(b)/);
if (m2 != null) console.log(m2[0], m2[1]);

const re3: RegExp = /b/;
const m3: string[] | null = re3.exec("abc");
if (m3 !== null) console.log(m3[0]);

// truthiness condition, not a null compare
const m4: string[] | null = "q".match(/q/);
if (m4) console.log(m4[0]);
const m5: string[] | null = "z".match(/nope/);
if (!m5) console.log("no match");

// --- preserved face: every one of these reads the table ---
const p1: string[] | null = "xabby".match(/ab*/);
console.log(p1);                       // bare X in a call argument

const p2: any = "xabby".match(/ab*/);
console.log(p2.index, p2.input);

const p3: any = "abc".match(/(?<y>b)/);
console.log(p3.index, p3.groups.y);

const re4: RegExp = /ab*/;
const p4: string[] | null = re4.exec("xabby");
console.log(p4);

// bare X escaping into another binding
const p5: string[] | null = "xabby".match(/ab*/);
const alias: string[] | null = p5;
console.log(alias);

// bare X escaping into a container
const p6: string[] | null = "xabby".match(/ab*/);
const box: (string[] | null)[] = [p6];
console.log(box[0]);

// optional chaining on the binding
const p7: any = "xabby".match(/ab*/);
console.log(p7?.index);

// a global regex advances lastIndex across exec calls; the shape
// still rides each result
const g: RegExp = /b/g;
console.log(g.exec("abcb"));
console.log(g.exec("abcb"));
