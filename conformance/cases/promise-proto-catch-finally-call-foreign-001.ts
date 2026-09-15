// The reified Promise.prototype.catch / .finally on receivers that are
// not Promise cells: §27.2.5.1 / §27.2.5.3 are receiver-generic Invokes.
var target: any = {};
target.then = function(a: any, b: any) { console.log("T", arguments.length, a, b, this === target); return 7; };
console.log(Promise.prototype.catch.call(target, 1, 2, 3));
console.log(Promise.prototype.finally.call(target, 1, 2, 3));
var f = function(){};
var r = Promise.prototype.finally.call(target, f);
console.log(r);

var Thenable: any = function(){};
Thenable.prototype.then = function() { return 9; };
console.log(Promise.prototype.finally.call(new Thenable()));
console.log(Promise.prototype.catch.call(new Thenable()));

(Boolean.prototype as any).then = function() { console.log("bool then", typeof this); };
Promise.prototype.catch.call(true);
(Number.prototype as any).then = function() { console.log("num then"); };
Promise.prototype.catch.call(34);

var poisoned = Object.defineProperty({}, "then", { get: function() { throw new RangeError("poison"); } });
try { Promise.prototype.catch.call(poisoned); console.log("no throw"); } catch (e) { console.log("poisoned getter:", e instanceof RangeError); }
var thrower = { then: function() { throw new RangeError("boom"); } };
try { Promise.prototype.catch.call(thrower); console.log("no throw"); } catch (e) { console.log("then throws:", e instanceof RangeError); }

var p: any = new Promise(function(){});
for (const v of [undefined, null, 1, "", true, {}]) {
  p.then = v;
  try { Promise.prototype.finally.call(p, function(){}); console.log("no throw"); } catch (e) { console.log("not callable:", typeof v, e instanceof TypeError); }
}
try { Promise.prototype.finally.call(3, function(){}); console.log("no throw"); } catch (e) { console.log("finally non-object:", e instanceof TypeError); }
try { Promise.prototype.catch.call(undefined); console.log("no throw"); } catch (e) { console.log("catch undefined:", e instanceof TypeError); }
