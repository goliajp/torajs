// §22.1.3.13 String.prototype.matchAll step 2.c — a pattern carrying a
// user @@matchAll runs it as Call(matcher, regexp, «O») and answers
// its value as-is; a getter that throws propagates, a present but
// non-callable matcher is the GetMethod TypeError, and an absent one
// falls back to RegExpCreate(ToString(regexp), "g").

// typed lane: object-literal pattern with a computed symbol key
let litCalls = 0;
let litThis: any = null;
let litArg: any = null;
const lit = {
  [Symbol.matchAll]: function (s: string) {
    litCalls += 1;
    litThis = this;
    litArg = s;
    return "lit-result";
  },
};
const r1: any = "abc".matchAll(lit as any);
console.log(r1, litCalls, litThis === lit, litArg);

// any lane: store into an empty object
var obj = {};
var calls = 0;
var seenThis: any = null;
var seenArgs: any = null;
var token = { tag: "token" };
obj[Symbol.matchAll] = function () {
  calls += 1;
  seenThis = this;
  seenArgs = arguments;
  return token;
};
const r2: any = "xyz".matchAll(obj);
console.log(r2 === token, calls, seenThis === obj, seenArgs.length, seenArgs[0]);

// an any-typed receiver takes the dynamic method lane
var recv: any = "pqr";
const r3: any = recv.matchAll(obj);
console.log(r3 === token, calls, seenThis === obj, seenArgs[0]);

// getter throws -> propagated unchanged
class Boom extends Error {}
var getterObj = {};
Object.defineProperty(getterObj, Symbol.matchAll, {
  get: function () {
    throw new Boom("getter");
  },
});
try {
  "a".matchAll(getterObj);
  console.log("no-throw");
} catch (e) {
  console.log(e instanceof Boom, (e as any).message);
}

// matcher throws -> propagated unchanged
var thrower = {};
thrower[Symbol.matchAll] = function () {
  throw new Boom("matcher");
};
try {
  "a".matchAll(thrower);
  console.log("no-throw");
} catch (e) {
  console.log(e instanceof Boom, (e as any).message);
}

// non-callable matcher -> TypeError, typed and dynamic receivers
var bad: any = {};
bad[Symbol.matchAll] = 7;
try {
  "a".matchAll(bad);
  console.log("no-throw");
} catch (e) {
  console.log(e instanceof TypeError);
}
try {
  recv.matchAll(bad);
  console.log("no-throw");
} catch (e) {
  console.log(e instanceof TypeError);
}

// null matcher -> fallback through ToString with the g flag
var nul: any = {};
nul[Symbol.matchAll] = null;
nul.toString = function () {
  return "\\d";
};
const r4: any = "a1b2c3".matchAll(nul);
const all: string[] = [];
for (const m of r4) {
  all.push(m[0]);
}
console.log(all.join(","));
