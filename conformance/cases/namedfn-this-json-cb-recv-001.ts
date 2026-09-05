// A promoted named fn in the JSON callback slots gets the holder as
// `this`: §25.5.1.1 InternalizeJSONProperty calls the reviver with the
// holder, §25.5.2.2 step 3 calls the replacer the same way. The plain
// forwarder hardwired a non-holder receiver, so the same program in
// the function-expression spelling and the named-declaration spelling
// disagreed.
function revive(k: string, v: any): any {
  const th = this as any;
  if (k === "") return v;
  return th && th.k === 7 ? "holder" : "other";
}

function replace(k: string, v: any): any {
  const th = this as any;
  if (k === "") return v;
  return th && th.k === 7 ? "holder" : "other";
}

console.log(JSON.stringify(JSON.parse('{"k":7}', revive)));
console.log(JSON.stringify({ k: 7 }, replace));

// The function-expression twin has answered this since RFC
// 20260808-json-parse-any blade 4 — both spellings now agree.
const reviveExpr = function (k: string, v: any): any {
  const th = this as any;
  if (k === "") return v;
  return th && th.k === 7 ? "holder" : "other";
};
console.log(JSON.stringify(JSON.parse('{"k":7}', reviveExpr)));

// A this-free reviver keeps the plain route and its own answer.
function plain(k: string, v: any): any {
  return k === "" ? v : "p" + String(v);
}
console.log(JSON.stringify(JSON.parse('{"k":7}', plain as any)));

// The same fn stays callable directly — the recv shell is minted per
// site, the declaration's own ABI is untouched.
console.log(String(revive("", 5)));

// A rebound `JSON` disarms the census: a user-written `parse` has no
// receiver channel, so its callback must keep the plain shim.
const bag = { parse: (t: string, f: any) => f("k", 7) };
console.log(String(bag.parse("x", revive)));
