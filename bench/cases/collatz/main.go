package main

import (
	"fmt"
	"os"
	"strconv"
)

func steps(n uint64) uint64 {
	var count uint64 = 0
	for n != 1 {
		if n&1 == 0 {
			n >>= 1
		} else {
			n = 3*n + 1
		}
		count++
	}
	return count
}

func main() {
	limit, _ := strconv.ParseUint(os.Args[1], 10, 64)
	var max uint64 = 0
	for i := uint64(1); i <= limit; i++ {
		s := steps(i)
		if s > max {
			max = s
		}
	}
	fmt.Println(max)
}
