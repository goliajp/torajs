// `globalThis.X` reads the global object's property (§9.4.2); a
// declaration of `X` in some scope changes the bare name there, never
// the property read. Outside that scope the read is the builtin as
// before; inside it, it still is.

function run(process: string): string {
  return process;
}
console.log(run("a"), globalThis.process.argv.length > 0);

function f(Math: number): number {
  return Math + globalThis.Math.floor(2.5);
}
console.log(f(1));

function g(process: string): boolean {
  return globalThis.process.argv.length > 0 && process.length === 1;
}
console.log(g("p"));

const h = (Math: number): number => globalThis.Math.max(Math, 2);
console.log(h(5), h(1));

try {
  throw 1;
} catch (JSON) {
  console.log(globalThis.JSON.stringify({ a: JSON }));
}

{
  const parseFloat = 3;
  console.log(parseFloat, globalThis.parseFloat("2.5"));
}
console.log(globalThis.parseFloat("1.25"));

function k(n: number): number {
  let Math = n * 2;
  for (let i = 0; i < 2; i++) Math += i;
  return globalThis.Math.abs(-Math);
}
console.log(k(3));

function q(isNaN: number): string {
  const inner = (): number => isNaN + 1;
  return inner() + ":" + globalThis.isNaN(isNaN);
}
console.log(q(4));

class C {
  m(Math: number): number {
    return globalThis.Math.floor(Math + 0.5);
  }
  static s(JSON: string): string {
    return globalThis.JSON.stringify(JSON);
  }
}
console.log(new C().m(2.7), C.s("q"));

function* gen(Math: number) {
  yield globalThis.Math.floor(Math);
  yield Math;
}
console.log([...gen(3.5)]);

function d(Math: number = 2, y: number = globalThis.Math.floor(Math + 0.5)): number {
  return Math + y;
}
console.log(d(), d(4.6));

const named = function isFinite(n: number): boolean {
  return globalThis.isFinite(n) && typeof isFinite === "function";
};
console.log(named(1), named(Infinity));

function sw(k: number): string {
  switch (k) {
    case 1: {
      const escape = "e";
      return escape + globalThis.escape("a b");
    }
    default:
      return globalThis.escape("c d");
  }
}
console.log(sw(1), sw(2));

async function as1(parseInt: number): Promise<number> {
  await null;
  return globalThis.parseInt("12") + parseInt;
}
as1(1).then((v) => console.log("async", v));
