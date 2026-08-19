// ES2025 Promise.try / §27.2.4.8 Promise.withResolvers through the
// receiver channel: `.call(Promise, ...)`, the builtin-heir
// class-object chain, and the detached TypeError. The thenable
// completion is created LAST: tr resolves it through PromiseResolve
// (one tick fewer than the spec capability.resolve absorption — the
// NewPromiseCapability(C) recorded follow-up), so cross-chain
// ordering before it would diverge from bun.
const t: any = (Promise as any).try;
t.call(Promise, (a: number, b: number) => a + b, 20, 22).then((v: any) => console.log("args", v));
t.call(Promise, () => {
  throw new Error("sync boom");
}).catch((e: any) => console.log("rejected", e.message));
t.call(Promise, 123).catch(() => console.log("noncallable rejects"));

class CP extends Promise<any> {}
(CP as any).try(() => {
  throw new Error("heir boom");
}).catch((e: any) => console.log("heir rejected", e.message));

const w: any = (Promise as any).withResolvers;
const wr = w.call(Promise);
wr.promise.then((v: any) => console.log("wcall", v));
wr.resolve(7);
const wr2 = (CP as any).withResolvers();
wr2.promise.then((v: any) => console.log("heir-wr", v));
wr2.resolve(9);

try {
  t(() => 1);
} catch {
  console.log("detached threw");
}

t.call(Promise, () => Promise.resolve("inner")).then((v: any) => console.log("absorbed", v));
