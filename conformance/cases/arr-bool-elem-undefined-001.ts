// A bool slot holds two states, so a read of a `boolean[]` that can
// answer `undefined` comes back as a tagged value instead — an
// out-of-range index, `at`, a `find` miss, `pop` / `shift` on an
// empty array (ES §10.4.2.1 / §23.1.3.8 / §23.1.3.22 / §23.1.3.25).
// The array's own slots are untouched, so a read that stays in range
// still hands back a plain bool.
function line(tag: string, f: () => unknown) {
  try {
    console.log(tag, f());
  } catch (e) {
    console.log(tag, "THREW", (e as Error).name);
  }
}

const bs: boolean[] = [true, false, true];
const empty: boolean[] = [];
const bs2: boolean[] = [true, false];

line("oob", () => bs[9]);
line("neg", () => bs[-1]);
line("at", () => bs.at(9));
line("find", () => bs.find((b) => b && false));
line("findLast", () => bs.findLast((b) => b && false));
line("pop", () => empty.pop());
line("shift", () => empty.shift());

line("typeof", () => typeof bs[9]);
line("eq-undef", () => bs.at(9) === undefined);
line("eq-null", () => bs.at(9) === null);
line("truthy", () => (bs.at(9) ? "yes" : "no"));
line("not", () => !bs.at(9));
line("box", () => {
  const a: any = bs.at(9);
  return a;
});
line("json", () => JSON.stringify({ b: bs[9] }));

// An un-annotated binding keeps the answer; so do an array literal,
// an object field and an `any` parameter.
const miss = bs[9];
line("let-unann", () => miss);
line("let-unann-typeof", () => typeof miss);
line("arr-lit", () => [bs[9]]);
line("obj-field", () => ({ on: bs[9] }));
function anyparam(v: any): string {
  return typeof v;
}
function tb(b: boolean): string {
  return b ? "T" : "F";
}
line("param-any", () => anyparam(bs[9]));

// In-range reads keep answering plain booleans, through every
// consumer that a bool has.
line("read", () => bs[0]);
line("read-false", () => bs[1]);
line("at-live", () => bs.at(0));
line("at-neg-live", () => bs.at(-1));
line("find-live", () => bs.find((b) => !b));
line("pop-live", () => bs2.pop());
line("if", () => {
  if (bs[0]) return "taken";
  return "not";
});
line("and", () => bs[0] && bs[2]);
line("or", () => bs[1] || bs[2]);
line("eq-true", () => bs[0] === true);
line("ternary", () => (bs[1] ? "t" : "f"));
line("annotated-let", () => {
  const x: boolean = bs[0];
  return x;
});
line("bool-param", () => tb(bs[0]) + tb(bs[1]));
line("push", () => {
  const cs: boolean[] = [];
  cs.push(bs[0]);
  cs.push(bs[1]);
  return cs;
});
line("assign", () => {
  const cs: boolean[] = [false, false];
  cs[0] = bs[0];
  return cs[0];
});
line("json-arr", () => JSON.stringify(bs));
line("map", () => bs.map((b) => !b));
line("filter", () => bs.filter((b) => b));
line("includes", () => bs.includes(bs[1]));
line("count", () => {
  let n = 0;
  for (let i = 0; i < bs.length; i++) if (bs[i]) n++;
  return n;
});
// Last: returning the captured array out of the arrow leaves it
// unusable for a later walk (a pre-existing bug, same on the
// binary before this change), so this face goes after the rest.
line("print-arr", () => bs);
