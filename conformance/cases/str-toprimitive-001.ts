// OrdinaryToPrimitive (ES 7.1.1.1) for object operands of
// ToString / ToNumber -- user toString/valueOf hooks run in hint
// order and the FIRST primitive result wins (undefined counts as
// primitive); both-objects throws a catchable TypeError.
// RFC 20260712-string-proto-cluster chunk C.

// toString returning undefined -> "undefined" (not [object Object]).
var o1 = { toString: function() {} };
console.log(JSON.stringify(String(o1)));
console.log(String(o1).indexOf(void 0));

// toString returning a number -> ToString of it.
var o2 = { toString: function() { return 42; } };
console.log(String(o2));

// no own toString -> inherited [object Object]; valueOf untouched.
var o3 = { valueOf: function() { return 7; } };
console.log(String(o3));

// toString answers an object -> falls to valueOf.
var o4 = { toString: function() { return {} as any; }, valueOf: function() { return "vo"; } };
console.log(String(o4));

// both answer objects -> catchable TypeError, both accessed.
var sawTs = false, sawVo = false;
var o5 = {
  toString: function() { sawTs = true; return {} as any; },
  valueOf: function() { sawVo = true; return {} as any; },
};
try { String(o5); console.log("no-throw"); } catch (e) { console.log("caught"); }
console.log(sawTs, sawVo);

// a throwing toString propagates.
var o6 = { toString: function() { throw "boom"; } };
try { String(o6); console.log("no-throw"); } catch (e) { console.log("caught:" + e); }

// ToNumber hint: valueOf first.
var o7 = { valueOf: function() { return 6; }, toString: function() { return "9"; } };
var a7: any = o7;
console.log(Number(a7));
console.log((a7 as any) * 2);

// any-lane string concat runs the hook too.
var a2: any = o2;
console.log("v=" + a2);

// typed-lane concat + template run the hook too (S138 mirror).
console.log("t=" + o2);
console.log(String(o2) + "!");

// own entry present but NOT callable -> skipped, next method runs
// (IsCallable probe; search A1_T9 shape).
var o8 = { valueOf: function() {}, toString: void 0 } as any;
console.log(JSON.stringify(String(o8)));
console.log(String(o8).indexOf("undef"));
var o9 = { toString: 42, valueOf: function() { return 3; } } as any;
console.log(String(o9));
