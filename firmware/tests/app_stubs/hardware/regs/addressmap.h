#pragma once
#include <stdint.h>
extern uint8_t test_flash_sector[4096];
#define XIP_BASE ((uintptr_t)test_flash_sector-(PICO_FLASH_SIZE_BYTES-4096u))
