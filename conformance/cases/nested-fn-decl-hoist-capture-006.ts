// A declaration that cannot become a value (a generic, an `arguments`
// reader) keeps only the peers it names as declarations; a peer that
// names it and reads the body's locals still becomes a value, in an
// async body and in a generator body alike.

async function asyncGeneric() {
  let n = 1;
  function id<T>(x: T): T { return x; }
  function k() { return id(n); }
  await null;
  n = 2;
  return k();
}
asyncGeneric().then(v => console.log("asyncGeneric", v));

async function asyncChain(base: number) {
  let n = base;
  function inc(x: number): number { return x + 1; }
  function wrap<T>(x: T): T { inc(0); return x; }
  function read(): number { return wrap(n) + inc(n); }
  await null;
  n += 10;
  return read();
}
asyncChain(3).then(v => console.log("asyncChain", v));

async function asyncArgs() {
  let n = 4;
  function count(): number { return arguments.length; }
  function use(): number { return count() + n; }
  await null;
  return use();
}
asyncArgs().then(v => console.log("asyncArgs", v));

function* genGeneric() {
  let n = 1;
  function id<T>(x: T): T { return x; }
  function k() { return id(n); }
  yield 0;
  n = 2;
  yield k();
  yield id(3);
}
for (const v of genGeneric()) console.log("genGeneric", v);

function* genChain(base: number) {
  let n = base;
  function inc(x: number): number { return x + 1; }
  function wrap<T>(x: T): T { inc(0); return x; }
  function read(): number { return wrap(n) + inc(n); }
  yield read();
  n += 10;
  yield read();
  yield wrap("w") + inc(1);
}
for (const v of genChain(3)) console.log("genChain", v);

function* genArgs() {
  let n = 4;
  function count(): number { return arguments.length; }
  function use(): number { return count() + n; }
  yield 0;
  n = 5;
  yield use();
}
for (const v of genArgs()) console.log("genArgs", v);
