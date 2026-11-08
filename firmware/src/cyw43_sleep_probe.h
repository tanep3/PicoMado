#pragma once
#include <stdint.h>
#include <stdbool.h>
typedef struct {
 uint32_t sleep,wake,success[2],failure[2],max_loops,last_us,duration_us,last_failure_us;
 int32_t last_read;
 bool before,after;
} pm_sleep_status;
void pm_sleep_get(pm_sleep_status *out);
