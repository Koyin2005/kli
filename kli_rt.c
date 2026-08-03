#include <stdio.h>
#include <inttypes.h>
#include <stdint.h>
#include <stdlib.h>
#include "kli_pal.h"
void kli_print_newline(void){
    kli_print_string("\n",1);
}

void kli_print_int(uint64_t value) {
    printf("%lld",value);
    fflush(stdout);
}
