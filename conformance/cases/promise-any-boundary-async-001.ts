// An async function declared `Promise<T>` whose body returns an `any`
// value resolves its promise with that value as it is (§27.7.5.3), so
// an `await` of a call reads what was returned, whatever `T` says.

async function af(x: any): Promise<number> {
  return x;
}

async function mixed(x: any, c: boolean): Promise<number> {
  if (c) return 1.5;
  return x;
}

async function str(x: any): Promise<string> {
  if (x === 0) return "zero";
  return x;
}

async function field(o: { v: any }): Promise<number> {
  return o.v;
}

async function typed(n: number): Promise<number> {
  return n * 2;
}

class K {
  async m(x: any): Promise<number> {
    return x;
  }
}

async function main() {
  const s: any = JSON.parse('"str"');
  const a = await af(s);
  console.log("plain", typeof a, a);
  const b = await mixed(s, false);
  console.log("mixed any", typeof b, b);
  const c = await mixed(s, true);
  console.log("mixed typed", typeof c, c + 1);
  const d = await str(0);
  console.log("string", typeof d, d, d.length);
  const e = await str(JSON.parse("7"));
  console.log("string any", typeof e, e);
  const f = await field({ v: "fv" });
  console.log("field", typeof f, f);
  const g = await new K().m(s);
  console.log("method", typeof g, g);
  const h = await typed(21);
  console.log("typed", typeof h, h);
  const all = await Promise.all([af(JSON.parse('"a"')), af(JSON.parse("2"))]);
  console.log("all", typeof all[0], all[0], typeof all[1], all[1]);
  af(s).then((v: number) => console.log("typed handler", typeof v, v));
}
main();
