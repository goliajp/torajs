// A module whose only top-level awaits sit in class declarations (a
// computed member key, an `extends` operand) awaits them in place like
// any other top-level await: each lets the jobs queued ahead of it run
// first (§15.7.14 evaluates both where the class stands).
const log: string[] = [];
let p = Promise.resolve();
for (let i = 1; i <= 5; i++) {
  p = p.then(() => {
    log.push("t" + i);
  });
}
class Base {
  hello() {
    return "base";
  }
}
const k = "m";
class C {
  [await Promise.resolve(k)]() {
    return 7;
  }
}
log.push("after C " + new C().m());
class D extends (await Promise.resolve(Base)) {}
log.push("after D " + new D().hello());
p.then(() => console.log(log.join(" ")));
export {};
