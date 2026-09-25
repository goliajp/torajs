// a captured number / boolean / string binding that is never written is
// snapshotted into the closure env; one written in any of these ways,
// before or after the capture, must still be shared live (es §9.1).
function forOfHead(): number {
  let x = 0;
  const f = (): number => x;
  for (x of [1, 2, 3]) {}
  return f();
}
function forInHead(): string {
  let k = "a";
  const f = (): string => k;
  for (k in { p: 1, q: 2 }) {}
  return f();
}
function destructure(): number {
  let a = 1;
  const f = (): number => a;
  [a] = [7];
  return f();
}
function objDestructure(): number {
  let a = 1;
  const f = (): number => a;
  ({ a } = { a: 9 });
  return f();
}
function compound(): number {
  let a = 1;
  const f = (): number => a;
  a += 4;
  return f();
}
function prefixInc(): number {
  let a = 1;
  const f = (): number => a;
  ++a;
  return f();
}
function logicalAssign(): number {
  let a = 0;
  const f = (): number => a;
  a ||= 5;
  return f();
}
function innerWrite(): number {
  let a = 1;
  const set = (): void => {
    a = 11;
  };
  const get = (): number => a;
  set();
  return get();
}
function varRedecl(): number {
  var a = 1;
  const f = (): number => a;
  var a = 3;
  return f();
}
function paramWrite(p: number): number {
  const f = (): number => p;
  p = p * 10;
  return f();
}
function readOnly(p: number, q: boolean): string {
  const f = (): string => p + ":" + q;
  return f();
}
function loopCapture(): string {
  const fs: (() => number)[] = [];
  for (let i = 0; i < 3; i++) fs.push(() => i);
  let out = "";
  for (const g of fs) out = out + g();
  const k = 5;
  const hs: (() => number)[] = [];
  for (let j = k; j < 7; j = j + 1) hs.push(() => j * k);
  for (const h of hs) out = out + "," + h();
  return out;
}
function hoisted(): number {
  const r = g();
  let late = 4;
  function g(): number {
    return typeof late === "number" ? 1 : 0;
  }
  return r + late;
}
function escaping(): () => number {
  const base = 40;
  return () => base + 2;
}
console.log(forOfHead(), forInHead(), destructure(), objDestructure(), compound());
console.log(prefixInc(), logicalAssign(), innerWrite(), varRedecl(), paramWrite(3));
console.log(readOnly(5, true), loopCapture(), escaping()());
try {
  console.log(hoisted());
} catch (e) {
  console.log("tdz", (e as Error).name);
}
