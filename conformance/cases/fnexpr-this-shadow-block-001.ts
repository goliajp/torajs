// A function expression reading `this`, held by a block's `const g`
// while another `g` in a nested block, a `for … of` head or a sibling
// block holds a method value: each call reads the binding it lexically
// reaches (§14.2.2), so those `g` are the method, run on `o`.

const obj2 = {
  tag: "obj2",
  m() {
    return this.tag;
  },
};
const o = { tag: "o" };

class C {
  tag = "c";
  run(): string {
    const f = function () {
      return this === undefined ? "undef" : this.tag;
    };
    const out: string[] = [];
    {
      const g = f;
      out.push(g.call(this));
      {
        const g = obj2.m;
        out.push(g.call(o));
      }
      for (const g of [obj2.m]) out.push(g.call(o));
    }
    {
      const g = obj2.m;
      [1].forEach(() => out.push(g.call(o)));
    }
    return out.join(",");
  }
}
console.log(new C().run());
