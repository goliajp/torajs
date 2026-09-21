// §27.2.5.4 steps 3-4 — a `then` handler that is not callable makes
// the reaction slot EMPTY, and the source value is forwarded. That
// is why asking the wrong question here is a silent wrong answer
// rather than a refusal: tr probed for a closure-cell (env, boxed
// entry) pair, so a Proxy or a class handler read as "not callable"
// and the chain forwarded as if nothing had been written. A
// `.then(proxyHandler)` fulfilled with the ORIGINAL value, and the
// `.catch` behind it never ran — all of it well-formed, none of it
// distinguishable from a program that meant to forward.
//
// "Callable" is §7.2.3. A shape with no pair now gets the same thunk
// the array kernels take, owned by the reaction record, which
// already stakes and drops the env it is handed.
//
// Receivers are `any`-typed throughout: the typed lane's checker
// still refuses an `any` handler on a `Promise<number>`, which is a
// separate gap in front of this one.
const out: string[] = [];
const log = (s: string): void => {
  out.push(s);
};
const dbl: any = new Proxy(((v: number) => v * 2) as any, {});
const trapped: any = new Proxy((() => 0) as any, {
  apply: (_t: any, _th: any, a: any) => a[0] * 3,
});
const handle: any = new Proxy(((e: any) => "h:" + e) as any, {});
const boom: any = new Proxy((() => {
  throw new Error("boom");
}) as any, {});

const r1: any = Promise.resolve(5);
const r2: any = Promise.resolve(5);
const r3: any = Promise.reject("x");
const r4: any = Promise.reject("x");
const r5: any = Promise.resolve(5);
const r6: any = Promise.resolve(5);
const r7: any = Promise.resolve(5);
const r8: any = Promise.resolve(5);
const r9: any = Promise.resolve(5);
class P extends Promise<any> {}
const r10: any = P.resolve(1);

Promise.all([
  r1.then(dbl).then((v: any) => log("then proxy      " + v)),
  r2.then(trapped).then((v: any) => log("then apply trap " + v)),
  r3.catch(handle).then((v: any) => log("catch proxy     " + v)),
  // A class is callable, so it is called — and §10.2.1 step 2 throws
  // inside the reaction, which rejects the derived promise. Being
  // ignored (and fulfilling with 5) was the bug.
  r5.then(class {} as any).then(
    (v: any) => log("then class      fulfilled " + v),
    (e: any) => log("then class      rejected " + (e instanceof TypeError)),
  ),
  // Genuinely not callable — the slot really is empty, and
  // forwarding is correct.
  r6.then(7).then((v: any) => log("then number     fwd " + v)),
  r7.then(undefined).then((v: any) => log("then undefined  fwd " + v)),
  r4.then(undefined, handle).then((v: any) => log("onRejected      " + v)),
  r8.finally(new Proxy((() => log("finally ran")) as any, {})).then((v: any) =>
    log("finally proxy   " + v)),
  // A throwing proxy handler rejects the derived promise.
  r9.then(boom).catch((e: any) => log("throwing proxy  " + e.message)),
  // The species path asks the same question in its own copy.
  r10.then(dbl).then((v: any) => log("species then    " + v)),
  // Plain closures, unmoved.
  (Promise.resolve(5) as any).then((v: any) => v * 2).then((v: any) =>
    log("plain           " + v)),
]).then(() => {
  out.sort();
  for (const line of out) console.log(line);
});
