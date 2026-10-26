// A computed key evaluated ahead of an await in the same object literal
// is converted to a property key right after the key expression runs
// (§13.2.5.4 ComputedPropertyName), before the next value and before the
// suspension, not when the literal is finally made. The .expected is the
// spec order (test262 computed-property-name-topropertykey-before-value-
// evaluation); bun converts each key after its value instead.

const log: string[] = [];
const P = (v: any) => Promise.resolve(v);
function key(name: string): any {
  return { toString() { log.push("key " + name); return name } };
}
function val(name: string, v: number) {
  log.push("val " + name);
  return v;
}

async function f() {
  const o: any = { [key("a")]: val("a", 1), [await P("b")]: val("b", 2) };
  log.push(JSON.stringify(o));
  const p: any = {
    [key("c")]: val("c", 3),
    [key("d")]: await P(4),
    [key("e")]: val("e", 5),
  };
  log.push(JSON.stringify(p));
}

async function* g() {
  const o: any = { [key("x")]: val("x", 1), y: await P(2), [key("z")]: 3 };
  log.push(JSON.stringify(o));
  yield 1;
}

async function main() {
  await f();
  log.push("|");
  for await (const v of g()) log.push("v" + v);
  console.log(log.join("; "));
}
main();
