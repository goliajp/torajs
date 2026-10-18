package main

import (
	"fmt"
	"os"
	"strconv"
)

type Opt struct {
	a    int64
	hasA bool
	b    int64
}

func makeOpt(i int64) Opt {
	if i%4 == 0 {
		return Opt{a: i, hasA: true, b: i}
	}
	return Opt{b: i}
}

func main() {
	n, _ := strconv.ParseInt(os.Args[1], 10, 64)
	var total int64 = 0
	var created int64 = 0
	for i := int64(0); i < n; i++ {
		o := makeOpt(i)
		if !o.hasA {
			o.a = 2
			o.hasA = true
			created++
		}
		o.b++
		a := int64(0)
		if o.hasA {
			a = o.a
		}
		total = total + a + o.b
	}
	fmt.Println(total, created)
}
