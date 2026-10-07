// A name that resolves to a parameter, a local or a capture is not the
// function declaration spelled the same, even over an escaped array
function show(v: number): string {
  return "G" + typeof v;
}

function viaParam(show: (v: number) => string, d: number[]): string {
  const u: any = d;
  u[1] = "s";
  return d.map(show).join(",");
}
console.log(viaParam((v: number) => "P", [1, 2, 3]));

function viaConst(d: number[]): string {
  const show = (v: number) => "C" + v;
  const u: any = d;
  u[1] = "s";
  return d.map(show).join(",");
}
console.log(viaConst([4, 5]));

function viaBlock(d: number[]): string {
  const u: any = d;
  u[0] = "s";
  {
    const show = (v: number) => "B" + v;
    return d.map(show).join(",");
  }
}
console.log(viaBlock([7, 8]));

function viaCapture(show: (v: number) => string) {
  return (d: number[]) => {
    const u: any = d;
    u[1] = "s";
    return d.map(show).join(",");
  };
}
console.log(viaCapture((v: number) => "K")([1, 2]));

function viaAny(show: (v: any) => string, d: any[]): string {
  return d.map(show).join(",");
}
console.log(viaAny((v: any) => "A" + v, [1, "s"]));

function viaForEach(show: (v: number) => void, d: number[]): void {
  const u: any = d;
  u[0] = "s";
  d.forEach(show);
}
viaForEach((v: number) => console.log("F"), [1, 2]);

function plain(d: number[]): string {
  return d.map(show).join(",");
}
console.log(plain([1, 2]));

const top = [1, 2];
const tu: any = top;
tu[0] = "s";
console.log(top.map(show).join(","));
