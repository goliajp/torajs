// A member write on a possibly-absent receiver compiles, evaluates its
// right-hand side, and only then raises the TypeError.
//
// §13.15.2 evaluates the reference, then the value, then PutValue —
// and §6.2.5.6 PutValue is where §7.1.20 ToObject throws for an absent
// base. So `o.a = side()` prints SIDE before the TypeError, unlike a
// method call (§13.3.6.1), which throws before its arguments.
//
// The same order governs §10.1.9.2 [[Set]]'s frozen check, which the
// struct store ran BEFORE the value: `Object.freeze(o); o.a = side()`
// threw without printing SIDE. Both checks now sit after the value,
// which is parked as a throw temp so either refusal releases it.
//
// Written out directly, never through a helper (r687).

class C {
  v: number = 3;
  s: string = "x";
}

function maybeO(): { a: number } | undefined {
  return Math.random() < 2 ? { a: 1 } : undefined;
}
function maybeC(): C | undefined {
  return Math.random() < 2 ? new C() : undefined;
}
function noneO(): { a: number } | undefined {
  return undefined;
}
function nullC(): C | null {
  return null;
}
function side(): number {
  console.log("SIDE");
  return 5;
}

function main(): void {
  const o: { a: number } | undefined = maybeO();
  o.a = 5;
  o.a += 2;
  console.log(o.a);
  const c: C | undefined = maybeC();
  c.v = 9;
  c.s = "yy";
  c.s += "z";
  console.log(c.v, c.s);

  const go: { a: number } | undefined = noneO();
  try {
    go.a = side();
    console.log("no throw");
  } catch (e) {
    console.log((e as Error).constructor.name);
  }
  const nc: C | null = nullC();
  try {
    nc.s = "q" + "r";
    console.log("no throw");
  } catch (e) {
    console.log((e as Error).constructor.name);
  }

  const f = { a: 1 };
  Object.freeze(f);
  try {
    f.a = side();
    console.log("no throw");
  } catch (e) {
    console.log((e as Error).constructor.name);
  }
  console.log(f.a);
}

main();
