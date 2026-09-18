// A local binding named like a built-in global or a runtime module
// namespace (`fs`, `process`, `Bun`, `Math`) is an ordinary local: an
// arrow that reads it captures it — the lexical environment shadows
// the global one (§9.1.1.1). The capture walk used to skip every
// global name unconditionally, so a lifted body reading a local `fs`
// died on `unknown ident fs` while the same program under any other
// name ran.
function viaModuleName(): number {
  const fs: (() => number)[] = [() => 1, () => 2];
  const g = () => fs.length + fs[1]();
  return g();
}
function viaProcess(): string {
  const process = { pid: 7, title: "local" };
  const g = () => process.title + process.pid.toString();
  return g();
}
function viaBun(): number {
  let Bun = 40;
  const g = () => Bun + 2;
  Bun = 41;
  return g();
}
function viaMath(): number {
  const Math = { floor: (x: number) => x * 10 };
  const g = () => Math.floor(4);
  return g();
}
function noShadow(): number {
  const g = () => Math.floor(4.7);
  return g();
}
console.log(viaModuleName(), viaProcess(), viaBun(), viaMath(), noShadow());
