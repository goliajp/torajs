// different: []float64 has no per-element type tag; main.ts reads an any[] and dispatches on both operands' tags
package main

import (
	"fmt"
	"os"
	"strconv"
)

func main() {
	n, _ := strconv.ParseInt(os.Args[1], 10, 64)
	pool, _ := strconv.ParseInt(os.Args[2], 10, 64)
	var arr []float64
	for j := int64(0); j < pool; j++ {
		arr = append(arr, float64(j)+0.5)
	}
	var total float64 = 0
	for i := int64(0); i < n; i++ {
		total = total + arr[i%pool]
	}
	fmt.Println(strconv.FormatFloat(total, 'f', -1, 64))
}
