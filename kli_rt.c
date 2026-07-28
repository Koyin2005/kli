#include <stdio.h>
#include <inttypes.h>

void kli_print_int(uint64_t value) {
    printf("%lld\n",value);
}

void kli_print_string(uint8_t *ptr, size_t len){
    printf("%.s\n",len,ptr);
}