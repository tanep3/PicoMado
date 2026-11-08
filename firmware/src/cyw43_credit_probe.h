#pragma once
#include <stdint.h>
#include <stdbool.h>
#include <stddef.h>
#define PM_CREDIT_CAP 16u
typedef struct {
 uint32_t at_us,wait_us,polls,data,control,event,error;
 int32_t result;
 uint16_t length;
 uint8_t kind,flow,credit,sequence,end_flow,end_credit,end_sequence,covered;
 bool changed,complete,timeout;
} pm_credit_row;
/* Caller holds the existing CYW43 lock. No pointers or contents escape. */
bool pm_credit_get(unsigned index,pm_credit_row *out,uint32_t *count,uint32_t *dropped);
