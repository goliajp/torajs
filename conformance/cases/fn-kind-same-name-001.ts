// A function declared in one function and an async / generator / async
// generator function of the same name declared in another keep their own
// kinds: the synchronous one returns its value, not a Promise.

function a1(k: number) {
  function inner() {
    return k;
  }
  return inner;
}
function a2(k: number) {
  async function inner() {
    return k;
  }
  return inner;
}
console.log(a1(1)());

// the async one parsed first
function b2(k: number) {
  async function pick() {
    return k + 10;
  }
  return pick;
}
function b1(k: number) {
  function pick() {
    return k + 20;
  }
  return pick;
}
console.log(b1(1)());

// generator and synchronous function of one name
function g1(k: number) {
  function step() {
    return k;
  }
  return step();
}
function g2(k: number) {
  function* step() {
    yield k;
    yield k + 1;
  }
  return [...step()];
}
console.log(g1(3), g2(4));

// async generator and synchronous function of one name
function h1(k: number) {
  function walk() {
    return k * 2;
  }
  return walk();
}
function h2() {
  async function* walk() {
    yield 6;
    yield 7;
  }
  return walk;
}
console.log(h1(5));

// async and async generator of one name
function m1() {
  async function feed() {
    return "m1";
  }
  return feed;
}
function m2() {
  async function* feed() {
    yield "m2";
  }
  return feed;
}

// two levels deep
function o1() {
  function mid() {
    function deep() {
      return 7;
    }
    return deep();
  }
  return mid();
}
function o2() {
  function mid() {
    async function deep() {
      return 8;
    }
    return deep();
  }
  return mid();
}
console.log(o1());

// a top-level synchronous function and a nested async one
function run() {
  return "top";
}
function o3() {
  async function run() {
    return "nested";
  }
  return run();
}
console.log(run());

// a top-level async function and a nested synchronous one
async function fetchIt() {
  return 1;
}
function p1() {
  function fetchIt() {
    return 2;
  }
  return fetchIt();
}
console.log(p1());

// called before the declaration, and recursive
function r1() {
  const v = fact(5);
  function fact(n: number): number {
    return n <= 1 ? 1 : n * fact(n - 1);
  }
  return v;
}
function r2() {
  return fact(4);
  async function fact(n: number): Promise<number> {
    return n <= 1 ? 1 : n * (await fact(n - 1));
  }
}
console.log(r1());

// names and kinds as values
const s1 = a1(0);
const s2 = a2(0);
console.log(s1.name, s2.name, typeof s1, typeof s2);
console.log(String(s1).startsWith("function inner"), String(s2).startsWith("async function inner"));

// an async arrow and a class method spelled like a synchronous function
function d1(k: number) {
  function f() {
    return k;
  }
  return f();
}
function d2(k: number) {
  const f = async () => k;
  return f();
}
class Box {
  async f() {
    return "box";
  }
}
console.log(d1(9));

// inside an arrow body and inside a method body
const viaArrow = () => {
  function tick() {
    return "arrow-sync";
  }
  return tick();
};
class Holder {
  go() {
    async function tick() {
      return "method-async";
    }
    return tick();
  }
}
console.log(viaArrow());

async function main() {
  console.log("a2", await a2(2)());
  console.log("b2", await b2(2)());
  for await (const v of h2()()) console.log("h2", v);
  console.log("m1", await m1()());
  for await (const v of m2()()) console.log("m2", v);
  console.log("o2", await o2());
  console.log("o3", await o3());
  console.log("fetchIt", await fetchIt());
  console.log("r2", await r2());
  console.log("d2", await d2(10));
  console.log("box", await new Box().f());
  console.log("holder", await new Holder().go());
}
main();
