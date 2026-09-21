// §27.2.5.4 step 3/4 — a reaction handler that is not callable is an
// EMPTY slot, not an error: PerformPromiseThen leaves it empty and
// the source value is forwarded. §27.2.5.1 `catch` and §27.2.5.3
// `finally` say the same of theirs.
//
// This is the exact opposite of what the Array callback methods say
// (§23.1.3.x: throw a TypeError), and the two rules meet in the same
// checker — which is why the decision reads the method and never the
// parameter type alone. See array-callback-not-callable-001 for the
// other side of the same sentence.
//
// tr's typed promise arms fix the handler's signature, so anything
// that was not a matching Function got refused. The any lane has
// answered all of these correctly since rotation 682; what was
// missing is the checker's permission to send the call there.
const log: any[] = [];
function done(): void {
  console.log(log.join("\n"));
}

const pr: any = new Proxy((v: any) => v * 2, {});
class K {}

Promise.resolve(7)
  .then(undefined)
  .then((v: any) => log.push("then undefined   " + v))
  .then(() => Promise.resolve(8).then({} as any))
  .then((v: any) => log.push("then object      " + v))
  .then(() => Promise.resolve(9).then(pr))
  .then((v: any) => log.push("then proxy       " + v))
  .then(() => Promise.resolve(10).then(K as any))
  .then(
    (v: any) => log.push("then class       ran:" + v),
    (e: any) => log.push("then class       threw:" + e.constructor.name),
  )
  // A null onFulfilled with a real onRejected: the empty slot
  // forwards, the rejection handler is untouched.
  .then(() => Promise.resolve(11).then(null as any, (e: any) => "no:" + e))
  .then((v: any) => log.push("then null, cb    " + v))
  .then(() => Promise.reject(new Error("m")).catch(undefined as any).catch((e: any) => e.message))
  .then((v: any) => log.push("catch undefined  " + v))
  .then(() => Promise.resolve(12).finally({} as any))
  .then((v: any) => log.push("finally object   " + v))
  // Rejections still travel past an empty onFulfilled slot.
  .then(() => Promise.reject(new Error("r")).then(undefined).catch((e: any) => e.message))
  .then((v: any) => log.push("reject past slot " + v))
  // Controls: a matching handler keeps its typed chain, and the
  // value it answers keeps its type.
  .then(() => Promise.resolve(13).then((v: number) => v + 1))
  .then((v: any) => log.push("typed chain      " + v))
  .then(() => Promise.resolve("s").then((v: string) => v + "!"))
  .then((v: any) => log.push("typed string     " + v))
  .then(done);
