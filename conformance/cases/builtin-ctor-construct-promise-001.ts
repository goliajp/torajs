// [[Construct]] of Promise through a first-class constructor VALUE
// (§27.2.3.1): a fresh base promise whose resolving functions the
// executor receives; an executor throw rejects (step 10); a
// non-callable executor is the step-2 TypeError — refused BEFORE the
// newTarget's prototype is read (step 3), which is what the
// test262 `get-prototype-abrupt-executor-not-callable` case pins.
const P: any = Promise;
const p = new P((res: any) => res(41));
console.log(p instanceof Promise, typeof p.then);
p.then((v: number) => console.log("resolved", v + 1));
new P((_: any, rej: any) => rej(new Error("boom"))).catch((e: any) => console.log("rejected", e.message));
new P(() => { throw new RangeError("in executor") }).catch((e: any) => console.log("caught", e.constructor.name));
new P((res: any) => { res(1); throw new Error("after resolve") }).then((v: number) => console.log("still resolved", v));
try { new P(1); console.log("no throw") } catch (e: any) { console.log("threw", e.constructor.name) }
try { new P(); console.log("no throw") } catch (e: any) { console.log("threw", e.constructor.name) }
const bound = (function () {}).bind(null);
Object.defineProperty(bound, "prototype", { get() { throw new SyntaxError("prototype read") } });
try { Reflect.construct(P, [], bound); console.log("no throw") } catch (e: any) { console.log("threw", e.constructor.name) }
console.log(Reflect.construct(P, [(r: any) => r(7)]) instanceof Promise);
console.log("sync done");
