#pragma once
#include <stdbool.h>
#include <stdint.h>

void pm_capture_set_context(uint32_t frame, const char *phase, uint32_t requested_hz);
bool pm_capture_ready(void);
void pm_capture_dump(void);
