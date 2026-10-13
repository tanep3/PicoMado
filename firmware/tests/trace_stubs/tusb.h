#pragma once
#include <stdbool.h>
#include <stdint.h>
bool tud_cdc_connected(void);
uint32_t tud_cdc_write_available(void);
uint32_t tud_cdc_write(const void *buffer,uint32_t length);
uint32_t tud_cdc_write_flush(void);
