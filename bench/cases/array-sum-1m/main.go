package main

import "fmt"

func main() {
	xs := make([]int64, 0)
	for i := int64(0); i < 10000000; i++ {
		xs = append(xs, i)
	}
	var sum int64 = 0
	for j := 0; j < len(xs); j++ {
		sum += xs[j]
	}
	fmt.Println(sum)
}
