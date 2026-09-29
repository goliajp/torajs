package main

import (
	"fmt"
	"os"
	"strconv"
)

func loopSum(xs []int64, f func(int64) int64) int64 {
	var sum int64 = 0
	for _, x := range xs {
		sum = sum + f(x)
	}
	return sum
}

func main() {
	count, _ := strconv.ParseInt(os.Args[1], 10, 64)
	var xs []int64
	for i := int64(0); i < count; i++ {
		xs = append(xs, i)
	}
	offset, _ := strconv.ParseInt(os.Args[2], 10, 64)
	add := func(x int64) int64 { return x + offset }
	fmt.Println(loopSum(xs, add))
}
