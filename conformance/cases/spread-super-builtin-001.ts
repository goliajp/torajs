// super.m(...rest) in a builtin-heritage subclass method (rotation
// 372) — the t262 subclass-receiver-methods forwarding idiom: the
// spread flavor of the §13.3.7.3 parent-prototype re-dispatch.
let hasCount = 0;
let keysCount = 0;
class MySet extends Set {
  has(...rest: any[]) {
    hasCount += 1;
    return super.has(...rest);
  }
  keys(...rest: any[]) {
    keysCount += 1;
    return super.keys(...rest);
  }
}
const s1 = new MySet([1, 2]);
console.log(s1.has(1), s1.has(9), hasCount);

// the union kernel reads [[SetData]] directly — overrides stay
// unconsulted
const s2 = new Set([2, 3]);
const combined = s1.union(s2);
console.log([...combined].length, combined instanceof Set, hasCount, keysCount);

// prefix arg ahead of the spread through the Map setter
class MyMap extends Map {
  set(k: any, ...rest: any[]) {
    return super.set(k, ...rest);
  }
}
const m = new MyMap();
m.set("a", 1);
console.log(m.get("a"), m.size);

// an unknown super name answers the spec TypeError, catchable
class MyBad extends Set {
  poke(...rest: any[]) {
    // @ts-ignore
    return super.nosuchmethod(...rest);
  }
}
try {
  new MyBad().poke(1);
} catch (e) {
  console.log("caught", e instanceof TypeError);
}
