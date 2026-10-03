package main

import (
	"fmt"
	"os"
	"strconv"
)

func divisorSum(n uint64) uint64 {
	var s uint64 = 0
	for d := uint64(1); d*d <= n; d++ {
		if n%d == 0 {
			s += d
		}
	}
	return s
}

func main() {
	limit, _ := strconv.ParseUint(os.Args[1], 10, 64)
	var total uint64 = 0
	for n := uint64(1); n <= limit; n++ {
		total += divisorSum(n)
	}
	fmt.Println(total)
}
