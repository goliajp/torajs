// A 0-arg handler ignores its argument slot. The scalar-inner typed
// lane admitted that shape long ago; the Array-inner lane
// (`Promise.all([…]).then(…)`) still demanded `(arr) => V`, so the
// ordinary `.then(() => done())` was a compile reject (test262
// allSettled/resolved-immed and siblings, rotation 651).
Promise.all([Promise.resolve(1), Promise.resolve(2)]).then(() => console.log("all done"));
Promise.allSettled([Promise.resolve(3)]).then(() => console.log("allSettled done"));
Promise.all([Promise.resolve("a")]).then(() => 7).then((v) => console.log("chained", v));
Promise.all([Promise.reject(new Error("no"))]).then(() => console.log("unreached"), () => console.log("rejected"));
const p: Promise<number[]> = Promise.all([Promise.resolve(4)]);
p.then(() => { console.log("typed binding"); });
