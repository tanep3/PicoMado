#pragma once
#include <stddef.h>
#include <stdint.h>
typedef unsigned int uint;
#define GPIO_OUT 1
#define GPIO_FUNC_SPI 1
void gpio_init(uint pin);
void gpio_set_dir(uint pin, int output);
void gpio_set_function(uint pin, int function);
void gpio_put(uint pin, int value);
void sleep_ms(unsigned milliseconds);
uint64_t time_us_64(void);
void tight_loop_contents(void);
