function g(x: number) { return x + 1 }
function h() { return g }
console.log(h()(41));
