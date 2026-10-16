// RegExp.prototype[@@match / @@replace / @@search / @@split] run their
// spec algorithms whenever the receiver could observe the difference
// from a plain RegExp: every Get / Call / ToString / ToLength step
// propagates its throw, and any object may be the receiver.
const log: string[] = [];
function t(name: string, f: () => any) {
  try {
    console.log(name, JSON.stringify(f()));
  } catch (e: any) {
    // engine messages differ; only the user's own are compared
    console.log(name, "threw", e.constructor.name, e instanceof Boom ? e.message : "");
  }
}
class Boom extends Error {}

function throwingExec(): any {
  const r: any = /./g;
  r.exec = function () {
    throw new Boom("exec");
  };
  return r;
}
t("match exec throws", () => throwingExec()[Symbol.match]("abc"));
t("replace exec throws", () => throwingExec()[Symbol.replace]("abc", "x"));
t("search exec throws", () => throwingExec()[Symbol.search]("abc"));

t("match flags getter throws", () => {
  const r: any = /./;
  Object.defineProperty(r, "flags", { get() { throw new Boom("flags"); } });
  return RegExp.prototype[Symbol.match].call(r, "a");
});
t("match global getter throws", () => {
  const r: any = /./;
  Object.defineProperty(r, "global", { get() { throw new Boom("global"); } });
  return RegExp.prototype[Symbol.match].call(r, "a");
});
t("exec returns a primitive", () => {
  const r: any = /./;
  r.exec = () => 1;
  return r[Symbol.match]("a");
});

// result properties are read after every exec call, in spec order
t("replace result reads", () => {
  const r: any = /./;
  r.exec = function () {
    log.push("exec");
    return {
      get length() { log.push("length"); return 2; },
      get 0() { log.push("0"); return "b"; },
      get 1() { log.push("1"); return "c"; },
      get index() { log.push("index"); return 1; },
      get groups() { log.push("groups"); return undefined; },
    };
  };
  const out = r[Symbol.replace]("abcd", "[$1$&$`$']");
  return [out, log.join(",")];
});
t("replace index getter throws", () => {
  const r: any = /./;
  r.exec = () => ({ length: 1, 0: "a", get index() { throw new Boom("index"); } });
  return r[Symbol.replace]("abc", "x");
});
t("replace groups null", () => {
  const r: any = /./;
  r.exec = () => ({ length: 1, 0: "a", index: 0, groups: null });
  return r[Symbol.replace]("abc", "x");
});
t("replace named group getter", () => {
  const r: any = /./;
  r.exec = () => ({ length: 1, 0: "a", index: 0, groups: { get n() { return "N"; } } });
  return r[Symbol.replace]("abc", "<$<n>>");
});

// an ordinary object is a receiver too
const fake = {
  flags: "g",
  lastIndex: 0,
  calls: 0,
  exec(s: string) {
    this.calls++;
    return this.calls <= 2 ? { 0: "x", length: 1, index: this.calls } : null;
  },
};
t("match on an object", () => RegExp.prototype[Symbol.match].call(fake, "abc"));
t("search on an object", () =>
  RegExp.prototype[Symbol.search].call({ lastIndex: 0, exec: () => ({ index: 7 }) }, "abc"));
t("split species throws", () => {
  const r: any = /b/;
  r.constructor = { [Symbol.species]: function () { throw new Boom("species"); } };
  return RegExp.prototype[Symbol.split].call(r, "abc");
});
t("lastIndex valueOf throws", () => {
  const r: any = /(?:)/g;
  r.exec = function () {
    this.lastIndex = { valueOf() { throw new Boom("lastIndex"); } };
    return { 0: "", length: 1, index: 0 };
  };
  return r[Symbol.match]("abc");
});
t("plain regexp still native", () => [
  /b/g[Symbol.match]("abcb"),
  /b/[Symbol.replace]("abc", "x"),
  /c/[Symbol.search]("abc"),
  /b/[Symbol.split]("abc"),
]);
