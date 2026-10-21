// A handler after `.catch(onErr)` / `.then(onOk, onErr)` called on a
// binding declared `Promise<T>` takes the source's value or the
// handler's, the same as when the receiver is a `Promise.resolve` mint.

const bad: Promise<number> = Promise.reject(new Error("x"));
bad.catch((e) => 42).then((v) => console.log("rejected catch", v + 1, typeof v));

const ok: Promise<number> = Promise.resolve(5);
ok.catch((e) => 42).then((v) => console.log("fulfilled catch", v + 1, typeof v));
ok.then((x) => x * 2, (e) => 0).then((v) => console.log("two legs", v + 1, typeof v));
bad.then((x) => x * 2, (e) => -1).then((v) => console.log("two legs rejected", v + 1));

const s: Promise<string> = Promise.resolve("s");
s.catch((e) => "caught").then((v) => console.log("string catch", v + "!", v.length));

const mixed: Promise<number> = Promise.reject(new Error("m"));
mixed.catch((e) => "str").then((v) => console.log("mixed", v, typeof v));
ok.catch((e) => "str").then((v) => console.log("mixed ok", v, typeof v));

ok.catch((e) => 1).then((v) => v * 3).then((w) => console.log("chain", w + 1));

async function body(): Promise<void> {
  const p: Promise<number> = Promise.reject(new Error("b"));
  const a = await p.catch((e) => 7);
  console.log("await catch", a + 1);
  const q: Promise<number> = Promise.resolve(8);
  const b = await q.then((x) => x + 1, (e) => 0);
  console.log("await two legs", b + 1);
  const c = await (await q.catch((e) => 0).then((v) => Promise.resolve(v * 2)));
  console.log("nested await", c + 1);
}
body();
