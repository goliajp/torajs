// a String side makes `+` a concat whatever the any side holds; the
// result is a string in every slot it lands in
const g: any = parseInt("3", 10);
const ga = "ke" + g;
function readGa(): string {
  return ga;
}
function local(): void {
  const os: { k: string }[] = [];
  const a = "of" + g;
  os.push({ k: a });
  const ys: string[] = [];
  ys.push(g + "!");
  console.log(os[0].k, ys[0], readGa());
}
local();

const vals: any[] = [3, 2.5, "s", true, null, undefined, 10n, [1, 2], { a: 1 }];
for (const v of vals) {
  const l = "L" + v;
  const r = v + "R";
  console.log(l, r, l.length, r.length);
}
const o: any = {
  valueOf() {
    return 7;
  },
  toString() {
    return "ts";
  },
};
console.log("o" + o, o + "o");
const d: any = new Date(0);
console.log(("d" + d).length > 5);
const sym: any = Symbol("q");
try {
  console.log("x" + sym);
} catch (e) {
  console.log(e instanceof TypeError);
}
const bad: any = {
  valueOf() {
    throw new Error("boom");
  },
};
try {
  console.log("y" + bad);
} catch (e) {
  console.log((e as Error).message);
}
let acc = "";
for (const v of vals) acc += v;
console.log(acc);
console.log(`a${g}b${g}`);
const s = "hello world";
const sub = s.slice(0, 5);
console.log(sub + g, g + sub);
