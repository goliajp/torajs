// ECMA-404 §2: a JSON text is one value with only whitespace around
// it; anything after the value is a SyntaxError, whatever the slot's
// annotation. Each text goes through an annotated slot and an
// unannotated one.
type P = {
  a: number;
  b: string;
};

function show(lane: string, s: string, f: () => string): void {
  try {
    console.log(lane, JSON.stringify(s), f());
  } catch (e) {
    console.log(lane, JSON.stringify(s), "err", e instanceof SyntaxError);
  }
}

const nums: string[] = ["4.5 x", "4.5", "4.5 \n\t\r", "4.5,", "4.5]", "4.5 4"];
for (const s of nums) {
  show("typed", s, () => {
    const x: number = JSON.parse(s);
    return String(x);
  });
  show("any", s, () => String(JSON.parse(s)));
}

const strs: string[] = ['"hi" x', '"hi"  ', '"hi""yo"'];
for (const s of strs) {
  show("typed", s, () => {
    const x: string = JSON.parse(s);
    return x;
  });
  show("any", s, () => String(JSON.parse(s)));
}

const bools: string[] = ["true x", "false ", "truex"];
for (const s of bools) {
  show("typed", s, () => {
    const x: boolean = JSON.parse(s);
    return String(x);
  });
  show("any", s, () => String(JSON.parse(s)));
}

const arrs: string[] = ["[1,2] x", "[1,2] ", "[1,2]]", "[1,2],[3]"];
for (const s of arrs) {
  show("typed", s, () => {
    const xs: number[] = JSON.parse(s);
    return xs.join("|");
  });
  show("any", s, () => JSON.stringify(JSON.parse(s)));
}

const objs: string[] = ['{"a":1,"b":"z"} x', '{"a":1,"b":"z"}\n', '{"a":1,"b":"z"}}'];
for (const s of objs) {
  show("typed", s, () => {
    const p: P = JSON.parse(s);
    return p.a + p.b;
  });
  show("any", s, () => JSON.stringify(JSON.parse(s)));
}
