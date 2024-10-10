#include <stdio.h>
#include <assert.h>
#include <stdint.h>
#include "bs.h"
#ifndef _BS_MULTIPLY_128_H_
#define _BS_MULTIPLY_128_H_


// // Z is the output and assumed to be ord_length 
// void bs_multiply_128(word_t x[128], word_t y[128], word_t *z);

void transpose_mul(uint128_t x[64], uint128_t y[64], uint128_t* z);

#endif
