// different: []string has no per-element type tag, and strconv.ParseFloat accepts a different grammar than ToNumber (no whitespace trim, no 0b / 0o prefix); same strings, one conversion per iteration
package main

import (
	"fmt"
	"os"
	"strconv"
)

func main() {
	n, _ := strconv.ParseInt(os.Args[1], 10, 64)
	pool, _ := strconv.ParseInt(os.Args[2], 10, 64)
	// each element is a heap string, as '' + j is in main.ts
	var arr []string
	for j := int64(0); j < pool; j++ {
		arr = append(arr, strconv.FormatInt(j, 10))
	}
	var total float64 = 0
	for i := int64(0); i < n; i++ {
		s := arr[i%pool]
		v, _ := strconv.ParseFloat(s, 64)
		total = total + v
	}
	fmt.Println(strconv.FormatFloat(total, 'f', -1, 64))
}
