// A return that reads a `catch` parameter, a `for … of` variable or a
// binding declared inside a `switch` case settles that binding's value
// as it is, whatever an outer binding of the same name holds.

async function caught(x: any) {
  let e = 0;
  try {
    throw x;
  } catch (e) {
    return e;
  }
}

async function iterated(xs: any) {
  let n = 0;
  for (const n of xs) return n;
  return n;
}

async function switched(k: any, x: any) {
  let n = 0;
  switch (k) {
    case 1: {
      let n = x;
      return n;
    }
  }
  return n;
}

caught("s").then((v) => console.log("then catch", typeof v, v));
iterated(["s"]).then((v) => console.log("then for-of", typeof v, v));
switched(1, "s").then((v) => console.log("then switch", typeof v, v));

async function main() {
  const a = await caught("t");
  console.log("await catch", typeof a, a);
  const b = await iterated(["t"]);
  console.log("await for-of", typeof b, b);
  const c = await switched(1, "t");
  console.log("await switch", typeof c, c);
  const d = await iterated([]);
  console.log("await empty", typeof d, d);
}
main();
