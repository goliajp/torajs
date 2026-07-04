// RFC 20260704 S5+ — array spread over an `any` source via the
// unified runtime iteration protocol (materialize helper): Map/Set
// iterator cells minted by the C4+ method-call surface, any-held
// arrays and strings, plus literal + spread mixing through the
// already-lowered-operand assembler (no double side effects).
const m: any = new Map();
m.set("a", 1);
m.set("b", 2);
const ks: any = [...m.keys()];
console.log(ks);
const vs: any = [...m.values()];
console.log(vs[0]);
console.log(vs[1]);
const es: any = [...m.entries()];
console.log(es[0][0]);
console.log(es[1][1]);
const s: any = new Set();
s.add(10);
s.add(20);
const sv: any = [...s.values()];
console.log(sv);
// any-held array + literal mix — spread source is a call-free ident,
// literals pack around it
const a: any = [1, 2];
const cp: any = [...a, 9];
console.log(cp);
const cp2: any = [0, ...a];
console.log(cp2);
// any-held string spreads per code unit
const str: any = "hi";
const chars: any = [...str];
console.log(chars);
// spread source is a call — must mint the iterator exactly once
let calls = 0;
const mk = (): any => {
  calls = calls + 1;
  return m.keys();
};
const once: any = [...mk()];
console.log(once);
console.log(calls);
// non-iterable spread throws catchably
try {
  const n: any = 5;
  const bad: any = [...n];
  console.log(bad);
} catch (err) {
  console.log("caught");
}
console.log("done");
