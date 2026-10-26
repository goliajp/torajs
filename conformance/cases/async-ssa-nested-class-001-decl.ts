// An async method of a class declared inside a function, a block, an
// arrow body or a static block suspends over its frame like a top-level
// one, whichever name the class is hoisted under.

function plain() {
  class C {
    async m() {
      await 0;
      return 1;
    }
  }
  return new C().m();
}

// two classes named C in two functions and one at the top level: the
// nested ones are hoisted under fresh names
function first() {
  class C {
    async m() {
      await 0;
      return "first";
    }
  }
  return new C().m();
}
function second() {
  class C {
    m() {
      return "second, sync";
    }
  }
  return new C().m();
}
class C {
  async m() {
    await 0;
    return "top";
  }
}

function statics() {
  class S {
    static async s() {
      await null;
      return "static";
    }
    async m(x: number) {
      const y = await Promise.resolve(x);
      return y + 1;
    }
  }
  return S;
}

// a class that reads its function's local stays where it is written
function capturing(base: number) {
  class K {
    v = base;
    async get(d: number) {
      const x = await Promise.resolve(d);
      return this.v + x;
    }
    static async twice(n: number) {
      await null;
      return n * 2 + base;
    }
  }
  return K;
}

const inArrow = () => {
  class E {
    async m() {
      for await (const v of [1, 2]) console.log("arrow step", v);
      return "arrow";
    }
  }
  return new E().m();
};

async function main() {
  console.log(await plain());
  console.log(await first());
  console.log(second());
  console.log(await new C().m());
  const S = statics();
  console.log(await S.s(), await new S().m(4));
  const K = capturing(10);
  console.log(await new K().get(5), await K.twice(3));
  {
    class D {
      async m() {
        await 0;
        throw new Error("from a block");
      }
    }
    try {
      await new D().m();
    } catch (e) {
      console.log("caught", (e as Error).message);
    }
  }
  console.log(await inArrow());
}

class Outer {
  static {
    class In {
      async m() {
        await 0;
        return "static block";
      }
    }
    new In().m().then((v) => console.log(v));
  }
}

main().then(() => console.log("done", typeof Outer));
