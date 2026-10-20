// `null` cast from `any` to a built-in object type (ECMA-262
// §27.2.1.3.2 step 8: a resolution that is not an Object fulfills,
// one job later).

const order: string[] = [];

async function main(anyNull: any) {
  const d = await (anyNull as Date);
  order.push("d");
  console.log(d === null);
  const m = await (anyNull as Map<string, number>);
  console.log(m === null);
}

async function tick() {
  order.push("t1");
  await null;
  order.push("t2");
}

main(null).then(() => console.log(order.join(",")));
tick();
