// `arguments` inside a function expression written inline as a face of
// a property descriptor literal: get / set / value, through
// Object.defineProperty, Object.defineProperties and
// Reflect.defineProperty

var setArgs: any;
var o1: any = {};
Object.defineProperty(o1, "p", {
  set: function (a: any) {
    setArgs = arguments;
  },
  configurable: true,
});
o1.p = 3;
console.log(setArgs.length, setArgs[0]);
o1.p = "x";
console.log(setArgs.length, setArgs[0], typeof setArgs);

Object.defineProperty(o1, "g", {
  get: function () {
    return arguments.length;
  },
});
console.log(o1.g);

Object.defineProperty(o1, "m", {
  value: function (a: any, b: any) {
    return arguments.length + ":" + arguments[0] + ":" + arguments[2];
  },
});
console.log(o1.m(10));
console.log(o1.m(10, 20, 30));

var seen: any[] = [];
var o2: any = {};
Object.defineProperties(o2, {
  a: {
    get: function () {
      return arguments.length;
    },
    enumerable: true,
  },
  b: {
    set: function (v: any) {
      seen.push(arguments.length, arguments[0]);
    },
  },
  c: {
    value: function () {
      var all: any[] = [];
      for (var i = 0; i < arguments.length; i++) all.push(arguments[i]);
      return all.join("|");
    },
  },
});
console.log(o2.a);
o2.b = 7;
o2.b = "s";
console.log(seen.join(","));
console.log(o2.c(1, 2, 3, 4));
console.log(o2.c());

var escaped: any;
var o3: any = {};
var ok = Reflect.defineProperty(o3, "q", {
  set: function (v: any) {
    escaped = arguments;
  },
  get: function () {
    return arguments.length;
  },
  configurable: true,
});
console.log(ok);
o3.q = 42;
console.log(escaped.length, escaped[0], Array.prototype.slice.call(escaped).join(","));
console.log(o3.q);
console.log(Reflect.set(o3, "q", 9), escaped[0]);

Reflect.defineProperty(o3, "r", {
  value: function () {
    return arguments.length;
  },
});
console.log(o3.r(1, 2, 3));

// a face that reads `this` as well as `arguments`
var rcv: any;
var thisArgs: any;
var o4: any = { tag: "o4" };
Object.defineProperty(o4, "s", {
  set: function (a: any) {
    thisArgs = arguments;
    rcv = this;
  },
});
o4.s = 5;
console.log(thisArgs.length, thisArgs[0], rcv === o4, rcv.tag);
var other: any = { tag: "other" };
console.log(Reflect.set(o4, "s", 6, other), thisArgs[0], rcv.tag);
Reflect.defineProperty(o4, "t", {
  get: function () {
    return this.tag + "/" + arguments.length;
  },
  set: function (v: any) {
    rcv = this;
    thisArgs = arguments;
  },
});
console.log(o4.t);
o4.t = "w";
console.log(rcv === o4, thisArgs.length, thisArgs[0]);
