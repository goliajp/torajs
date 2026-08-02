// §15.8.1 await-context legal faces: top-level await, async function
// bodies, async arrows (ident + paren form), and an async object
// method must all keep working after non-async bodies started
// rejecting await at parse time.
async function f(): Promise<number> {
  const v: any = await Promise.resolve(3);
  return v;
}
console.log(await f());

const g: any = async x => (await Promise.resolve(x)) + 1;
console.log(await g(4));

const h: any = async (x: any) => {
  const v: any = await Promise.resolve(x);
  return v * 2;
};
console.log(await h(5));

const obj: any = {
  async m(x: any): Promise<number> {
    return (await Promise.resolve(x)) + 10;
  },
};
console.log(await obj.m(6));

const top: any = await Promise.resolve("top");
console.log(top);
