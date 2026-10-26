// An async arrow that a literal eval answers is an async function value:
// called, it answers a promise, and its awaits (one in a class key among
// them) suspend against the job queue.
const log: string[] = [];
let p = Promise.resolve();
for (let i = 1; i <= 4; i++) {
  p = p.then(() => {
    log.push("t" + i);
  });
}
const five: any = eval("async () => 5;");
const two: any = eval("async () => { await null; log.push('two'); return 2; };");
const klass: any = eval("async () => class { [await 'k'] = 1 };");
log.push(typeof five);
five().then((v: any) => log.push("five " + v));
two().then((v: any) => log.push("two " + v));
klass().then((C: any) => log.push("klass " + Object.keys(new C()).join(",")));
p.then(() => console.log(log.join(" ")));
