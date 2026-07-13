// ToNumber(position) on a no-primitive object throws TypeError (S332 family)
const noCoerce = Object.create(null);
try { "abc".charAt(noCoerce); console.log("bad-no-throw"); } catch (e) { console.log((e as Error).name); }
try { "abc".charCodeAt(noCoerce); console.log("bad-no-throw"); } catch (e) { console.log((e as Error).name); }
try { "abc".codePointAt(noCoerce); console.log("bad-no-throw"); } catch (e) { console.log((e as Error).name); }
try { "abc".at(noCoerce); console.log("bad-no-throw"); } catch (e) { console.log((e as Error).name); }
try { "abc".repeat(noCoerce); console.log("bad-no-throw"); } catch (e) { console.log((e as Error).name); }
try { "abc".slice(noCoerce); console.log("bad-no-throw"); } catch (e) { console.log((e as Error).name); }
try { "abc".padStart(noCoerce); console.log("bad-no-throw"); } catch (e) { console.log((e as Error).name); }
// valueOf IS observable and its result is used
let calls = 0;
const pos: any = { valueOf: function () { calls++; return 1; } };
console.log("abc".charAt(pos), calls);
console.log("abc".charCodeAt(pos), calls);
// any-tier receiver takes the same path
const anyS: any = "abc";
try { anyS.charCodeAt(noCoerce); console.log("bad-no-throw"); } catch (e) { console.log((e as Error).name); }
console.log(anyS.charAt(pos), calls);
// coerce matrix: plain object bridges to builtin toString (NaN -> 0),
// null-proto inherits nothing (TypeError), valueOf result is used
const plain: any = {};
console.log("plain:", "abc".charCodeAt(plain));
const withV: any = { valueOf: function () { return 2; } };
console.log("withV:", "abc".charCodeAt(withV));
