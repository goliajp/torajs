xs = []
i = 0
while i < 10000000:
    xs.append(i)
    i += 1
total = 0
j = 0
while j < len(xs):
    total += xs[j]
    j += 1
print(total)
