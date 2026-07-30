#include <stdio.h>
#include <inttypes.h>
#include <stdint.h>
#include <stdlib.h>

void kli_print_newline(void){
    printf("\n");
    fflush(stdout);
}

void kli_print_int(uint64_t value) {
    printf("%lld",value);
    fflush(stdout);
}

void kli_print_string(uint8_t *ptr, size_t len){
    printf("%.*s",len,ptr);
    fflush(stdout);
}
