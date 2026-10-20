// A rejection reason keeps its fraction at a handler declaring a
// `number` parameter (ECMA-262 §27.2.4.6, §27.2.5.1): a reason
// `Promise.reject` takes from JSON text and from an `any` field, and
// one an async function throws from an `any` binding, each read by
// `.catch((e: number) => …)`, and the same reasons caught by `await`.

const src: any = { x: 3.25 };

async function thrower(): Promise<number> {
  const h: any = JSON.parse("4.75");
  if (h > 0) throw h;
  return 1;
}

async function main() {
  await Promise.reject(JSON.parse("2.5")).catch((e: number) => console.log("json", e * 2));
  await Promise.reject(src.x).catch((e: number) => console.log("field", e * 2));
  await thrower().catch((e: number) => console.log("thrown", e * 2));
  const p: Promise<number> = Promise.reject(JSON.parse("1.5"));
  await p.then(
    (v: number) => console.log("value", v),
    (e: number) => console.log("handler", e + 1),
  );
  try {
    await thrower();
  } catch (e) {
    console.log("caught", (e as number) + 1);
  }
}
main();
