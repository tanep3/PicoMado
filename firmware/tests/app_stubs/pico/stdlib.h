#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
typedef unsigned int uint;
typedef uint64_t absolute_time_t;
#define GPIO_IN 0
#define PICO_FLASH_SIZE_BYTES (2u*1024u*1024u)
absolute_time_t get_absolute_time(void);
uint32_t to_ms_since_boot(absolute_time_t time);
void stdio_init_all(void);
void gpio_init(uint pin);
void gpio_set_dir(uint pin, int direction);
void gpio_pull_up(uint pin);
bool gpio_get(uint pin);
void sleep_ms(unsigned milliseconds);

uint64_t time_us_64(void);

static inline int getchar_timeout_us(uint32_t timeout){(void)timeout;return -1;}

uint get_core_num(void);
uint __get_current_exception(void);
