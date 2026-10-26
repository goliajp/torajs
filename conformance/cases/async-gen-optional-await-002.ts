// A parenthesized optional chain called in an async generator keeps the
// chain's reference (§13.3.9.1): with a nullish base the keys past the
// link are not evaluated, the arguments still are, and the call throws
// its TypeError after them (§13.3.6.1); with a present base the method
// is called on the object it was read from. Keys and arguments that
// await suspend in their place.

const log: string[] = [];
function at(s: string): string {
  log.push(s);
  return s;
}
function tick(s: string): Promise<string> {
  log.push("eval " + s);
  return Promise.resolve(s);
}
function err(e: any): string {
  return e instanceof TypeError ? "TypeError" : String(e);
}
async function* calls(o: any, k1: string) {
  try {
    (o?.[await tick("k")])(at("x1"));
  } catch (e) {
    log.push("c1 " + err(e));
  }
  try {
    (o?.a[await tick("k2")])(at("x2"));
  } catch (e) {
    log.push("c2 " + err(e));
  }
  try {
    (o?.[k1][await tick("k3")])(at("x3"));
  } catch (e) {
    log.push("c3 " + err(e));
  }
  try {
    (o?.a[await tick("k4")].m)(at("x4"));
  } catch (e) {
    log.push("c4 " + err(e));
  }
  try {
    (o?.a[at("k5")])(await tick("x5"));
  } catch (e) {
    log.push("c5 " + err(e));
  }
  try {
    (o?.a?.[await tick("k2")])(at("x6"), await tick("x7"));
  } catch (e) {
    log.push("c6 " + err(e));
  }
  yield 0;
}
function method(tag: string) {
  return function (this: any, v: string) {
    log.push(tag + " on " + this.tag + " " + v);
  };
}
const obj: any = {
  tag: "obj",
  k: method("k"),
  a: {
    tag: "a",
    k2: method("k2"),
    k4: { tag: "k4", m: method("m") },
    k5: method("k5"),
  },
  p: { tag: "p", k3: method("k3") },
};
async function main() {
  for (const o of [undefined, null, obj, { a: null }]) {
    for await (const _ of calls(o, "p")) {
    }
    console.log(log.join(" | "));
    log.length = 0;
  }
}
main();
