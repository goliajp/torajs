// A string method over an any receiver hands a RegExp to its protocol
// method (§22.1.3.13 step 2, §22.2.6.8). The link drops the protocol
// algorithms only from a program whose RegExps all stay plain; an own
// `exec` defined on one makes @@match call it (§22.2.7.1 step 2), so
// this program must keep them.
const re = /a/;
Object.defineProperty(re, "exec", {
  value: (s: string) => {
    console.log("exec", s);
    return null;
  },
});
const s: any = "banana";
console.log(s.match(re));
console.log(s.search(re));
console.log(s.replace(re, "o"));
const plain = /n/g;
console.log(s.match(plain));
