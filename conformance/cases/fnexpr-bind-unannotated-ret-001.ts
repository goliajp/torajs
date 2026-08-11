var D = function (x: number) {
  return x * 2;
};
var BD = D.bind(null, 21);
console.log(BD());
var BZ = D.bind(null);
console.log(BZ(7));
var S = function (a: number, b: string) {
  return b + a;
};
var BS = S.bind(null, 5);
console.log(BS("v"));
var E = function (x: number): number {
  return x + 1;
};
var BE = E.bind(null, 41);
console.log(BE());
