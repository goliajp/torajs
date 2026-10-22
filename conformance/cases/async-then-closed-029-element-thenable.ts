// An array element stored under a loop counter becomes a thenable: its
// `then` is installed under a key built at run time, so the program is
// not closed, and an await of the element calls it (ECMA-262 §27.2.1.3.2).

type Box = { a: number };

const k = ["th", "en"].join("");
const xs: Box[] = [];
for (let i = 0; i < 3; i = i + 1) {
  xs[i] = { a: i };
}
let calls = 0;
(xs[1] as any)[k] = (res: (v: number) => void) => {
  calls = calls + 1;
  res(40 + calls);
};

async function main() {
  for (let i = 0; i < xs.length; i++) {
    const v = await xs[i];
    console.log(i, typeof v, (v as any) === xs[i] ? "self" : String(v));
  }
  console.log(calls);
}

main();
