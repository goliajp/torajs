// A typed `.then` / `.catch` handler handed a value its parameter lane
// cannot hold. The promise is typed `Promise<number>` but settles from
// an `any`, so the value it carries is whatever the `any` held, and
// §27.2.5.4 calls the handler with it as it is: the body sees a string
// in `v: number`, `typeof` answers "string", and `v + 1` concatenates.
// Before, the reaction converted the value into the lane — a string
// became NaN, `true` became 1, and a number handed to a `string` lane
// was read as a string cell (a crash).

const str: any = "str";
const yes: any = true;
const num: any = 41;

async function main() {
  const p1: Promise<number> = Promise.resolve(str);
  await p1.then((v: number) => {
    console.log("str->number", typeof v, v);
  });

  const p2: Promise<number> = Promise.resolve(yes);
  await p2.then((v: number) => {
    console.log("bool->number", typeof v, v);
  });

  const p4: Promise<boolean> = Promise.resolve(str);
  await p4.then((v: boolean) => {
    console.log("str->boolean", typeof v, v);
  });

  const p5: Promise<string> = Promise.resolve(num);
  await p5.then((v: string) => {
    console.log("number->string", typeof v, v);
  });

  // a value that fits still takes the typed lane
  const p6: Promise<number> = Promise.resolve(num);
  await p6.then((v: number) => {
    console.log("fits", typeof v, v + 1);
  });

  // the twin's return settles the derived promise
  const p7: Promise<number> = Promise.resolve(str);
  const r7 = await p7.then((v: number) => v + 1).then((r: any) => r);
  console.log("ret", typeof r7, r7);

  // a capturing handler runs its twin on its own captures
  const tag = "cap";
  const p8: Promise<number> = Promise.resolve(str);
  await p8.then((v: number) => {
    console.log(tag, typeof v, v);
  });

  // a throw in the twin rejects the derived promise
  const p9: Promise<number> = Promise.resolve(str);
  await p9
    .then((v: number) => {
      throw new Error("bad " + v);
    })
    .catch((e: any) => {
      console.log("caught", e.message);
    });

  // the two-handler form, both legs
  const p10: Promise<number> = Promise.resolve(str);
  await p10.then(
    (v: number) => {
      console.log("two-ok", typeof v, v);
    },
    () => {},
  );
  const p11: Promise<number> = Promise.reject(str);
  await p11.then(
    () => {},
    (e: number) => {
      console.log("two-err", typeof e, e);
    },
  );

  // a catch handler whose lane the reason does not fit
  const p12: Promise<number> = Promise.reject(str);
  await p12.catch((e: number) => {
    console.log("catch", typeof e, e);
  });
}

main();
