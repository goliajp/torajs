// ECMA-404 / §25.5.1: JSON has no single quotes, trailing commas,
// comments, \x escapes, short \u escapes or unescaped control
// characters, and its only bare words are true / false / null. Each
// text goes through an annotated slot and an unannotated one.
function show(lane: string, s: string, f: () => string): void {
  try {
    console.log(lane, JSON.stringify(s), f());
  } catch (e) {
    console.log(lane, JSON.stringify(s), "err", e instanceof SyntaxError);
  }
}

const strs: string[] = [
  "'a'", '"a\tb"', '"a\nb"', '"\\u12"', '"\\u12G4"', '"\\u00e9"', '"\\x41"',
  '"\\u0041\\/"', '"abc', '"\\a"',
];
for (const s of strs) {
  show("typed", s, () => {
    const x: string = JSON.parse(s);
    return x;
  });
  show("any", s, () => String(JSON.parse(s)));
}

const arrs: string[] = ["[1,]", "[,1]", "[1 2]", "[1]//c", "/*c*/[1]", "[1,/*c*/2]", "[]", "[ ]"];
for (const s of arrs) {
  show("typed", s, () => {
    const xs: number[] = JSON.parse(s);
    return xs.join("|");
  });
  show("any", s, () => JSON.stringify(JSON.parse(s)));
}

type P = {
  a: number;
};
const objs: string[] = ['{"a":1,}', "{'a':1}", "{a:1}", '{"a" 1}', '{"a":1 "b":2}', '{"a":1}'];
for (const s of objs) {
  show("typed", s, () => {
    const p: P = JSON.parse(s);
    return String(p.a);
  });
  show("any", s, () => JSON.stringify(JSON.parse(s)));
}

const words: string[] = ["tru", "nul", "True", "false", "undefined", "falsey"];
for (const s of words) {
  show("typed", s, () => {
    const b: boolean = JSON.parse(s);
    return String(b);
  });
  show("any", s, () => String(JSON.parse(s)));
}
