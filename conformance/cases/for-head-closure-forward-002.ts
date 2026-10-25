// A `for` head's lexical bindings, and a block's, exist from the list's
// entry (§14.7.4.2, §14.2.2): a closure made before a declaration has
// run names the list's binding, never an outer one of the same name,
// and reading one early is a dead-zone read (§9.1.1.1.6), `typeof`
// included (§13.5.1.2).
const k = "outer";

const top: (number | string)[] = [];
for (const g = () => k, k = 1; ; ) {
  top.push(g());
  break;
}
{
  const g = () => k;
  const k = 3;
  top.push(g());
}

function headShadow(): string {
  const k = "s";
  const out: number[] = [];
  for (const g = () => k, k = 2; ; ) {
    out.push(g());
    break;
  }
  for (let g = () => k, k = 5; k < 7; k++) out.push(g(), k);
  return out.join(",");
}

function blockShadow(): string {
  const k = "s";
  {
    const g = () => k * 2;
    const k = 4;
    return String(g());
  }
}

function bodyShadow(): number {
  const g = () => k + 1;
  const k = 9;
  return g();
}

function nested(): string {
  const k = "s";
  const out: number[] = [];
  for (const g = () => k, k = 2; ; ) {
    {
      const k = 7;
      const h = () => k;
      out.push(g(), h());
    }
    break;
  }
  return out.join(",");
}

function name(e: any): string {
  return e.constructor.name;
}

function deadZone(): string {
  const k = "s";
  const out: string[] = [];
  try {
    for (const g = () => k, x = g(), k = 1; ; ) {
      out.push("no throw " + x + k);
      break;
    }
  } catch (e: any) {
    out.push(name(e));
  }
  try {
    for (let x = k, k = 1; ; ) {
      out.push("no throw " + x + k);
      break;
    }
  } catch (e: any) {
    out.push(name(e));
  }
  try {
    for (let x = typeof k, k = 1; ; ) {
      out.push("no throw " + x + k);
      break;
    }
  } catch (e: any) {
    out.push(name(e));
  }
  try {
    for (let g = () => typeof j, x = g(), j = 1; ; ) {
      out.push("no throw " + x + j);
      break;
    }
  } catch (e: any) {
    out.push(name(e));
  }
  return out.join(",");
}

async function asyncDeadZone(): Promise<string> {
  await null;
  const out: string[] = [];
  try {
    for (let g = () => j, x = g(), j = 1; ; ) {
      out.push("no throw " + x + j);
      break;
    }
  } catch (e: any) {
    out.push(name(e));
  }
  for (const g = () => j, j = 7; ; ) {
    await null;
    out.push(String(g()));
    break;
  }
  return out.join(",");
}

console.log("top", top.join(","), k);
console.log("headShadow", headShadow());
console.log("blockShadow", blockShadow());
console.log("bodyShadow", bodyShadow());
console.log("nested", nested());
console.log("deadZone", deadZone());
asyncDeadZone().then((s) => console.log("asyncDeadZone", s));
