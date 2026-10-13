#pragma once
#include <stddef.h>
#define PICO_UNIQUE_BOARD_ID_SIZE_BYTES 8
void pico_get_unique_board_id_string(char *id,size_t size);
