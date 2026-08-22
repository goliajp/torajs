// §10.2.4 / §13.3.6 — an object-literal method has a [[HomeObject]],
// so `super.m(…)` and `super[k](…)` read off GetPrototypeOf(home) and
// invoke with the CALL SITE's `this`, not with the home.
const parent: any = {
  getThis() { return this; },
  get This(): any { return this; },
  tag: "p",
  greet(n: string) { return "hi " + n + "/" + this.tag; },
};
const obj: any = {
  tag: "o",
  m() {
    return [
      super["getThis"]() === obj,
      super["This"] === obj,
      super.getThis() === obj,
      super.tag,
      super.greet("x"),
      super["greet"]("y"),
    ];
  },
  arrowed() { const f = () => super.greet("a"); return f(); },
};
Object.setPrototypeOf(obj, parent);
console.log(JSON.stringify(obj.m()));
// An arrow inherits the enclosing method's home (§8.3.4).
console.log(obj.arrowed());
// The home never moves; the receiver follows the call site.
const g = obj.m;
console.log(JSON.stringify(g.call({ tag: "z" })));
