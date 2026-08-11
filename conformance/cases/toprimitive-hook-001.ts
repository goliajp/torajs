// @@toPrimitive protocol — hint delivery, result acceptance,
// GetMethod semantics, and the §21.4.2.1 new Date(value) branches.
var calls = 0;
var seen: any[] = [];
var spy: any = {};
spy[Symbol.toPrimitive] = function () {
  calls++;
  seen.push(arguments.length);
  seen.push(arguments[0]);
  return 42;
};
console.log(Number(spy)); // hint "number"
console.log(String(spy)); // hint "string"
console.log(spy + 1); // hint "default"
console.log(spy == 42); // hint "default"
console.log(calls);
console.log(seen.join(","));

// object answer -> TypeError (catchable)
var bad: any = {};
bad[Symbol.toPrimitive] = function () {
  return {};
};
try {
  Number(bad);
  console.log("no-throw");
} catch (e: any) {
  console.log("caught:" + (e instanceof TypeError));
}

// hook throws -> propagates
var boom: any = {};
boom[Symbol.toPrimitive] = function () {
  throw new Error("boom");
};
try {
  String(boom);
  console.log("no-throw2");
} catch (e: any) {
  console.log("caught2:" + e.message);
}

// undefined answer is a primitive -> accepted
var u: any = {};
u[Symbol.toPrimitive] = function () {
  return undefined;
};
console.log(Number(u)); // NaN
console.log(String(u)); // "undefined"

// non-callable hook -> TypeError
var nc: any = {};
nc[Symbol.toPrimitive] = 5;
try {
  Number(nc);
  console.log("no-throw3");
} catch (e: any) {
  console.log("caught3:" + (e instanceof TypeError));
}

// null hook -> fall through to valueOf
var nul: any = { valueOf: function () { return 7; } };
nul[Symbol.toPrimitive] = null;
console.log(Number(nul)); // bun: TypeError? or 7 — oracle decides

// new Date(value) runs value's @@toPrimitive with hint "default"
var dv: any = {};
var dhint = "";
dv[Symbol.toPrimitive] = function (h: any) {
  dhint = h;
  return 0;
};
var d = new Date(dv);
console.log(dhint + ":" + d.getTime());

// hook answers a STRING -> parses (not ToNumber)
var sv: any = {};
sv[Symbol.toPrimitive] = function () {
  return "2020-01-02T03:04:05.000Z";
};
console.log(new Date(sv).getTime()); // parse of the ISO string

// Date argument copies [[DateValue]]
var base = new Date(1234567890123);
console.log(new Date(base as any).getTime()); // 1234567890123

// plain object without hook still ToNumbers through valueOf
console.log(new Date({ valueOf: function () { return 111; } } as any).getTime());

// string variable (not literal) parses
var s: any = "2021-03-04T00:00:00.000Z";
console.log(new Date(s).getTime());

// number variable keeps ms semantics
var n: any = 5000;
console.log(new Date(n).getTime());

// class with computed [Symbol.toPrimitive] member
class P {
  [Symbol.toPrimitive](hint: any): any {
    if (hint === "number") return 7;
    return "seven";
  }
}
var p: any = new P();
console.log(Number(p)); // 7
console.log(`${p}`); // seven (template = ToString -> hint string)
console.log(p + 0); // "seven0"? default hint -> "seven" + 0

// hook on object used in loose equality (default hint)
var q: any = {};
q[Symbol.toPrimitive] = function (h: any) {
  return h === "default" ? 9 : -1;
};
console.log(q == 9);
