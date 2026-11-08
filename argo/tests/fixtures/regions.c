int walk(int n)
{
    int value = 0;
    value += 1;
    for (int i = 0; i < n; i++) {
        if (i & 1)
            continue;
        else
            value += i;
    }
    while (value < n) {
        value++;
        break;
    }
    do {
        value--;
    } while (value > n);
    switch (n) {
    case 0:
    case 1:
        value++;
    case 2:
        value += 2;
        break;
    default:
        return -1;
    }
    {
        value += 3;
    }
    return value;
}
