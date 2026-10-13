#pragma once
#include <stddef.h>
#include <stdint.h>
#include <stdbool.h>
typedef unsigned int uint;
#define GPIO_IN 0
typedef uint64_t absolute_time_t;
enum gpio_drive_strength { GPIO_DRIVE_STRENGTH_2MA,GPIO_DRIVE_STRENGTH_4MA,
    GPIO_DRIVE_STRENGTH_8MA,GPIO_DRIVE_STRENGTH_12MA };
#define GPIO_OUT 1
#define GPIO_FUNC_SIO 5
#define GPIO_FUNC_SPI 1
void gpio_init(uint pin);
void gpio_set_dir(uint pin, int output);
void gpio_set_function(uint pin, int function);
void gpio_put(uint pin, int value);
bool gpio_get(uint pin);
bool gpio_get_dir(uint pin);
int gpio_get_function(uint pin);
enum gpio_drive_strength gpio_get_drive_strength(uint pin);
bool gpio_is_pulled_up(uint pin);
bool gpio_is_pulled_down(uint pin);
void gpio_pull_up(uint pin);
void stdio_init_all(void);
absolute_time_t get_absolute_time(void);
uint32_t to_ms_since_boot(absolute_time_t time);
void sleep_ms(unsigned milliseconds);
uint64_t time_us_64(void);
void tight_loop_contents(void);
