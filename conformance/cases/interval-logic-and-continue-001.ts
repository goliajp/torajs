// a digit loop that continues on a short-circuit `&&`: the true arm
// sees the code unit inside [48, 57]
function parseDigits(src: string): number {
  let i = 0;
  let v = 0;
  while (i < src.length) {
    const d = src.charCodeAt(i);
    if (d >= 48 && d <= 57) {
      v = v * 10 + (d - 48);
      i = i + 1;
    } else {
      break;
    }
  }
  return v;
}
console.log(parseDigits("7"));
console.log(parseDigits("1000000"));
console.log(parseDigits("77x77"));
console.log(parseDigits("/"));
console.log(parseDigits(":"));
console.log(parseDigits("9007199254740993"));
console.log(parseDigits("99999999999999999999"));
console.log(parseDigits("é1"));

// the `&&` in the loop condition itself
function countDigits(src: string): number {
  let i = 0;
  while (i < src.length && src.charCodeAt(i) >= 48 && src.charCodeAt(i) <= 57) {
    i = i + 1;
  }
  return i;
}
console.log(countDigits("12345"));
console.log(countDigits("12a45"));
console.log(countDigits(""));
console.log(countDigits("a"));

// a decided-both-ways shape: the false arm of `&&` must not narrow
function classify(src: string): number {
  let low = 0;
  let high = 0;
  let digit = 0;
  for (let i = 0; i < src.length; i++) {
    const d = src.charCodeAt(i);
    if (d >= 48 && d <= 57) {
      digit = digit + 1;
    } else if (d < 48) {
      low = low + 1;
    } else {
      high = high + (d - 57);
    }
  }
  return low * 1000000 + digit * 1000 + high;
}
console.log(classify("a1 b2 c3"));
console.log(classify("!!!"));
console.log(classify("zzz"));
