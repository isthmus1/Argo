int g(int);
int f(int a) {
  int *p; int i = 0; int t;
  if (a) return -1;
  p = &a;
  int n = g(a);
  t = n + i;
  return t + *p;
}
