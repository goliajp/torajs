// A handler on the promise of an async function declared Promise<T>
// whose body returns an `any` value receives that value as it is, and
// so does the next handler in the chain.

async function af(s: any): Promise<number> {
  return s;
}

async function proved(): Promise<number> {
  return 41;
}

af("str")
  .then((v) => ({ a: v }))
  .then((w) => console.log(typeof w.a, w.a));

af("x").then((v) => console.log(typeof v, v));

proved()
  .then((v) => ({ a: v + 1 }))
  .then((w) => console.log(typeof w.a, w.a));
