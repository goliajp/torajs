function probe(s: string): void {
  for (let t of s.split(" ")) {
    console.log(t.charCodeAt(0), t.charCodeAt(), t.charCodeAt(1), t.charCodeAt(-1), t.charCodeAt(99));
  }
}
probe("ab c 7");
probe("");
probe("日本 語x");
let u: string = "héllo wörld";
for (let t of u.split(" ")) {
  console.log(t.charCodeAt(0), t.charCodeAt(1));
}
let s2: string = "x y";
for (let t of s2.split(" ")) {
  console.log(t.charCodeAt(0.9), t.charCodeAt(0.2));
}
