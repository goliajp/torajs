package main

import (
	"fmt"
	"strconv"
)

func main() {
	m := make(map[string]int64)
	var n int64 = 100_000
	for i := int64(0); i < n; i++ {
		j := i % 4096
		var key string
		if i%2 == 0 {
			key = "key" + strconv.FormatInt(j, 10)
		} else {
			key = "ключ" + strconv.FormatInt(j, 10)
		}
		m[key] += i
	}
	var total int64 = 0
	for _, v := range m {
		total += v
	}
	fmt.Println(len(m), total)
}
