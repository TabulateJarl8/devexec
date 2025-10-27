#include <stdio.h>

// simple demo program that writes text to TTY0
// run with `cat a.out > /dev/exec`
int main() {
  FILE *f = fopen("/dev/pts/0", "w");
  fprintf(f, "Hello world\n");
  fclose(f);
}
