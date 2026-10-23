// Promise.race / Promise.any over an array that reached a
// `Promise<number>[]` slot from an `any` value: the winner is what the
// first element to settle settled with (§27.2.4.5.3, §27.2.4.3.1), a
// string, an object or undefined included, not a number read at the
// width the annotation names.
function strFirst(): any { return ["s", Promise.resolve(1)]; }
function numFirst(): any { return [7.5, Promise.resolve("x")]; }
function objFirst(): any { return [{ k: 1 }, Promise.resolve(2)]; }
function undefFirst(): any { return [undefined, Promise.resolve(3)]; }
function rejFirst(): any { return [Promise.reject(1), "late"]; }
async function main() {
  const a: Promise<number>[] = strFirst();
  const ra = await Promise.race(a);
  console.log("race s", ra, typeof ra);
  const aa = await Promise.any(a);
  console.log("any s", aa, typeof aa);
  const b = numFirst() as Promise<number>[];
  const rb = await Promise.race(b);
  console.log("race n", rb, typeof rb);
  console.log("any n", await Promise.any(b));
  const c: Promise<number>[] = objFirst();
  const rc = await Promise.race(c);
  console.log("race o", JSON.stringify(rc), typeof rc);
  const ac = await Promise.any(c);
  console.log("any o", JSON.stringify(ac), typeof ac);
  const d = undefFirst() as Promise<number>[];
  const rd = await Promise.race(d);
  console.log("race u", rd, typeof rd);
  const ad = await Promise.any(d);
  console.log("any u", ad, typeof ad);
  const e: Promise<number>[] = rejFirst();
  const ae = await Promise.any(e);
  console.log("any skip-rej", ae, typeof ae);
  const x = await Promise.race(strFirst() as Promise<number>[]);
  console.log("inline", x, typeof x, x + 1);
  Promise.race(a).then((v) => console.log("then race", v, typeof v));
  Promise.any(c).then((v) => console.log("then any", JSON.stringify(v)));
}
main();
