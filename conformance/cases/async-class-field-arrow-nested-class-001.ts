// An instance field's initializer arrow that declares a class with an
// async method, in a class with no constructor: the nested method is a
// function of its own and suspends at its await.

const log: string[] = [];
class X {
  f = () => {
    class D {
      async m(v: number) {
        log.push("m start " + v);
        const r = await Promise.resolve(v * 2);
        log.push("m end " + r);
        return r;
      }
    }
    return new D();
  };
}

async function main() {
  const d = new X().f();
  const p = d.m(5);
  log.push("after call");
  log.push("got " + (await p));
  console.log(log.join("; "));
}
main();
