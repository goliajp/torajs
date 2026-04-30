def trial(i):
    try:
        raise Exception(i)
    except Exception as e:
        return e.args[0]


total = 0
for i in range(100_000):
    total = total + trial(i)
print(total)
