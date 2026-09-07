// §7.1.17 ToString takes every value. The typed spelling of a BigInt,
// Map, Set or RegExp had no arm here, so `String(10n)` was a compile
// error ("String(BigInt) coercion not yet supported") while
// `String(10n as any)` answered "10" off the very kernel the typed
// spelling now boxes into.
const big = 10n;
const neg = -7n;
const map = new Map([[1, 2]]);
const set = new Set([1]);
const re = /ab+c/gi;

console.log(String(big), String(neg));
console.log(String(map), String(set), String(re));
console.log(`<${big}> <${neg}> <${map}> <${set}> <${re}>`);

// The any spelling answers the same, which is the point.
const anyBig: any = big;
const anyRe: any = re;
console.log(String(anyBig) === String(big), `${anyRe}` === `${re}`);
