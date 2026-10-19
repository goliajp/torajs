// a narrowed instance a call set to null, converted: serialized,
// interpolated, or read as a number, none of which runs its methods
// on null (§7.1.17 ToString, §7.1.4 ToNumber, §25.5.2.1 stringify)
class P {
  n: number;
  constructor(n: number) { this.n = n }
  toString(): string { return "P" + this.n }
  valueOf(): number { return this.n }
}
let q: P | null = new P(1);
function f(): boolean { q = null; return true }
function run(label: string, t: () => void): void {
  try { t() } catch (e) { console.log(label, (e as Error).constructor.name) }
  q = new P(2);
}

run("stringify", () => { if (q !== null && f()) console.log(JSON.stringify(q)) });
function ser(p: P): string { return JSON.stringify(p) }
run("stringify param", () => { if (q !== null && f()) console.log(ser(q)) });
run("stringify ok", () => { if (q !== null) console.log(ser(q)) });

run("template", () => { if (q !== null && f()) console.log(`v=${q}`) });
function show(p: P): string { return `w=${p}` }
run("template param", () => { if (q !== null && f()) console.log(show(q)) });
run("template ok", () => { if (q !== null) console.log(show(q)) });

run("valueOf", () => { if (q !== null && f()) console.log(Math.max(q as any, -1)) });
function num(p: P): number { return Math.max(p as any, -1) }
run("valueOf param", () => { if (q !== null && f()) console.log(num(q)) });
run("valueOf ok", () => { if (q !== null) console.log(num(q)) });
console.log("end");
