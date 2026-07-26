// A spread source shorter than the callee's parameter count leaves
// the tail reading past the end of the source, which answers
// `undefined` (ES §10.4.2.1) exactly as the same read does anywhere
// else. A synthesized length guard used to throw there instead —
// written when reading past the end of most element families threw
// too, so it stood in front of a hole rather than describing a rule.
function line(tag: string, f: () => unknown) {
  try { console.log(tag, f()); } catch (e) { console.log(tag, "THREW", (e as Error).name); }
}
function n3(a: number, b: number, c: number): string { return `${a}|${b}|${c}`; }
function withdef(a: number, b: number = 7, c: number = 8): string { return `${a}|${b}|${c}`; }
function strs(a: string, b: string): string { return `${a}|${b}`; }
function dates(a: Date, b: Date): string { return `${typeof a}|${typeof b}`; }
function objs(a: { v: number }, b: { v: number }): string { return `${typeof a}|${typeof b}`; }
const arrow = (a: number, b: number): string => `${a}|${b}`;

const n0: number[] = [];
const n1: number[] = [5];
const n2: number[] = [5, 6];
const s1: string[] = ["x"];
const d1: Date[] = [new Date(0)];
const o1: { v: number }[] = [{ v: 1 }];
const a1: any[] = ["z"];

line("def-full", () => withdef(...[1, 2, 3]));
line("def-short", () => withdef(...n1));
line("def-empty", () => withdef(...n0));
line("plain-1of3", () => n3(...n1));
line("plain-2of3", () => n3(...n2));
line("arrow-short", () => arrow(...n1));
line("str-short", () => strs(...s1));
line("date-short", () => dates(...d1));
line("obj-short", () => objs(...o1));
line("any-short", () => strs(...a1));
line("prefix-def", () => withdef(1, ...n1));
