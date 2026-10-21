// A handler after a two-leg `.then` / `.catch` whose legs settle a
// string, object or array on one side and undefined on the other reads
// undefined as undefined, and null as null.

function hs(x: number): string {
  return "s" + x;
}
function hu(e: any): undefined {
  return undefined;
}

const bad: Promise<number> = Promise.reject(new Error("x"));
bad.then(hs, hu).then((v) => console.log("rejected", v, v === undefined));
const good: Promise<number> = Promise.resolve(2);
good.then(hs, hu).then((v) => console.log("fulfilled", v, v === undefined));
bad.then((x) => "s" + x, (e): undefined => undefined).then((v) => console.log("closure", v, v === undefined));

const sr: Promise<string> = Promise.reject(new Error("y"));
sr.catch((e): undefined => undefined).then((v) => console.log("catch", v, v === undefined));
const sf: Promise<string> = Promise.resolve("hi");
sf.catch((e): undefined => undefined).then((v) => console.log("catch passes", v, v === undefined));

const objP: Promise<{ a: number }> = Promise.reject(new Error("o"));
objP.catch((e): undefined => undefined).then((v) => console.log("object", v === undefined ? "u" : v.a));
const arrP: Promise<number[]> = Promise.reject(new Error("o"));
arrP.catch((e) => null).then((v) => console.log("array null", v, v === null));
arrP.catch((e): undefined => undefined).then((v) => console.log("array undefined", v, v === undefined));

async function body(): Promise<void> {
  const s1: Promise<string> = Promise.reject(new Error("y"));
  const v = await s1.catch((e): undefined => undefined);
  console.log("await catch", v, v === undefined);
  const s2: Promise<string> = Promise.resolve("hi");
  const w = await s2.catch((e): undefined => undefined);
  console.log("await catch passes", w, w === undefined);
  const s3: Promise<number> = Promise.reject(new Error("n"));
  const x = await s3.then(hs, hu);
  console.log("await two legs", x, x === undefined);
}
body();
