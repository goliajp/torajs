package main

import (
	"fmt"
	"os"
	"strconv"
	"strings"
)

func next(s int64) int64 {
	t := (s * 48271) % 2147483647
	if t == 0 {
		return 1
	}
	return t
}

var words = []string{
	"alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta",
	"iota", "kappa", "lambda", "mu", "nu", "xi", "omicron", "pi",
}

func main() {
	target, _ := strconv.ParseInt(os.Args[1], 10, 64)
	rounds, _ := strconv.ParseInt(os.Args[2], 10, 64)
	seed, _ := strconv.ParseInt(os.Args[3], 10, 64)

	var total int64
	for r := int64(0); r < rounds; r++ {
		var b strings.Builder
		for int64(b.Len()) < target {
			seed = next(seed)
			b.WriteString(words[seed&15])
			b.WriteByte(' ')
		}
		s := b.String()
		total += int64(len(s)) + int64(s[len(s)>>1])
	}
	fmt.Println(total)
}
