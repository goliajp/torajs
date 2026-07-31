// Iterator.zip — proposal-joint-iteration (刀 5b). No bun/node
// reference implements it (probed 2026-07-31); acceptance is this
// hand-derived expectation diffed against tr run AND the AOT binary.
const z1: any = Iterator.zip([[1, 2, 3], [4, 5]]);
for (const row of z1) {
  console.log(row[0], row[1]);
}
const z2inputs: any = [[1, 2], ["a"]];
const z2: any = Iterator.zip(z2inputs, { mode: "longest", padding: ["z", "p"] });
for (const row of z2) {
  console.log(row[0], row[1]);
}
const z3: any = Iterator.zip([[1], [2]], { mode: "strict" });
console.log(z3.next().value[1], z3.next().done);
const z4: any = Iterator.zip([[1], [2, 3]], { mode: "strict" });
z4.next();
let threw = false;
try {
  z4.next();
} catch (e) {
  threw = true;
}
console.log(threw);
console.log((Iterator.zip([]) as any).next().done);
const noArg: any = undefined;
let threw2 = false;
try {
  Iterator.zip(noArg);
} catch (e) {
  threw2 = true;
}
console.log(threw2);
const badMode: any = { mode: "long" };
let threw3 = false;
try {
  Iterator.zip([], badMode);
} catch (e) {
  threw3 = true;
}
console.log(threw3);
const strEl: any = ["ab"];
let threw4 = false;
try {
  Iterator.zip(strEl);
} catch (e) {
  threw4 = true;
}
console.log(threw4);
const strObjInputs: any = [new String("ab"), [1, 2]];
const z5: any = Iterator.zip(strObjInputs);
for (const row of z5) {
  console.log(row[0], row[1]);
}
function* g() {
  yield 10;
  yield 20;
  yield 30;
}
const mixedInputs: any = [g(), [7, 8]];
const z6: any = Iterator.zip(mixedInputs).map((r: any) => r[0] + r[1]);
console.log([...z6].join(","));
