// resolve(promise) adopts the inner promise's state (§27.2.1.3.2);
// reject never adopts; [[AlreadyResolved]] locks the first settle.
// All inner promises here are already settled at resolve() time, so
// the reactions fire in attach order (no extra thenable-job hop).

// adopt an already-fulfilled promise
const a: any = Promise.withResolvers();
a.resolve(Promise.resolve(42));
a.promise.then((v: any) => console.log("A adopt-fulfilled:", v));

// adopt a rejected promise -> the rejection propagates
const c: any = Promise.withResolvers();
c.resolve(Promise.reject("rej-inner"));
c.promise.catch((e: any) => console.log("C adopt-rejected:", e));

// [[AlreadyResolved]]: first resolve(promise) wins, later resolve no-ops
const d: any = Promise.withResolvers();
d.resolve(Promise.resolve("first"));
d.resolve("second");
d.promise.then((v: any) => console.log("D already-resolved-lock:", v));

// reject never adopts -> rejects with the promise object itself
const inner = Promise.resolve("x");
const f: any = Promise.withResolvers();
f.reject(inner);
f.promise.catch((e: any) => console.log("E reject-no-adopt-identity:", e === inner));
