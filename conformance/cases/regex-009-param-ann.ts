// RegExp type annotation surface: const / param / return positions.
// `RegExp` is the TS spelling for the regex value type; a regex value
// flows through an annotated function parameter and return slot with
// normal ARC ownership.

const re: RegExp = /ban/;
console.log(re.test("banana"));

function testWith(r: RegExp, s: string): boolean {
  return r.test(s);
}
console.log(testWith(/a+/, "caaat"));
console.log(testWith(/z/, "caaat"));
console.log(testWith(re, "urban"));

function pick(flag: boolean): RegExp {
  if (flag) {
    return /^x/;
  }
  return /y$/;
}
const chosen: RegExp = pick(true);
console.log(chosen.test("xray"), chosen.test("box"));
console.log(pick(false).test("toy"));

// annotated regex in an array
const res: RegExp[] = [/one/, /two/];
console.log(res[0].test("someone"), res[1].test("someone"));
