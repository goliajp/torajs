// A generator declared or written inside a function body is a value
// that boxes into an `any` slot, whichever lowering the body takes
const H = {
  show(label: any, value: any) {
    console.log(label, typeof value);
  },
  define(obj: any, key: any, value: any) {
    Object.defineProperty(obj, key, {
      get() {
        return value;
      },
    });
  },
};

async function suspends() {
  async function* gen() {
    for (let i = 0; i < 3; i++) {
      yield Promise.resolve(i * 2);
    }
  }
  H.show("suspends", gen);
  const items: any = {};
  H.define(items, Symbol.asyncIterator, gen);
  const out = await Array.fromAsync(items);
  console.log(out.join(","));
  const again: number[] = [];
  for await (const x of gen()) again.push(x);
  console.log(again.join(","));
}

async function noAwait() {
  async function* gen() {
    yield 1;
  }
  H.show("noAwait", gen);
}

function plain() {
  const g = function* () {
    yield 5;
  };
  H.show("plain", g);
  const xs: number[] = [];
  for (const x of g()) xs.push(x);
  console.log(xs.join(","));
}

await suspends();
await noAwait();
plain();
