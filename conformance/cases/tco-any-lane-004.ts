// A strict tail call through an `any` callee the caller's frame holds
// (a local, or a call's result): the callee drops the only other
// holder of itself, makes enough closures of its own size to reuse
// the memory it would sit in were it already released, and then reads
// what it captured. The frame's hold has to last until the call
// returns.
function churn(): any {
  const keep: any[] = [];
  for (let i = 0; i < 2000; i++) {
    const tag = "junk" + i;
    keep.push((x: any): any => x + tag);
  }
  return keep.length;
}

function mk(holder: any, key: string, base: number): any {
  const label = "cap-" + base;
  return function (n: any): any {
    holder[key] = null;
    churn();
    return label + ":" + (n + base);
  };
}

// the callee is a local the frame holds
const box: any = { f: null };
box.f = mk(box, "f", 41);
function viaLocal(): any {
  const h: any = box.f;
  return h(1);
}
console.log("local", viaLocal());

// the callee is the result of a call
const store: any = {
  g: null,
  get(): any {
    return this.g;
  },
};
store.g = mk(store, "g", 7);
function viaResult(): any {
  return store.get()(2);
}
console.log("result", viaResult());

// the same through a chain of such calls
const chain: any = { f: null };
function viaChain(n: any): any {
  if (n === 0) return "chain done";
  chain.f = function (m: any): any {
    const was = chain.f;
    chain.f = null;
    churn();
    return typeof was + " " + viaChain(m);
  };
  const h: any = chain.f;
  return h(n - 1);
}
console.log(viaChain(3));
