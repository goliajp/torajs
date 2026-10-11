const __o: any = {}; Object.defineProperty(__o, "q", { value: 1 });
var count;
count = 0;
for (const { length, 0: c } in "abcdef") { console.log(length, c, "" + count++); }
count = 0;
for (const { length, 0: c } of "012345") { console.log(length, c, "" + count++); }
