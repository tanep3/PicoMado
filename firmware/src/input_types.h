#pragma once
#include <stdbool.h>
#include <stdint.h>
#include <limits.h>
#include <string.h>

/* A completed semantic operation, not a GPIO number or a held button. Zero is
 * invalid, so an uninitialized request never accidentally means SUB/POWER. */
typedef struct { uint8_t value; } pm_operation;
typedef enum { PM_OP_SUB=1, PM_OP_UP, PM_OP_POWER, PM_OP_DOWN } pm_operation_kind;
#define PM_OPERATION_MASK 0x0fu
static inline bool pm_operation_valid(pm_operation op){return op.value>=PM_OP_SUB&&op.value<=PM_OP_DOWN;}
static inline unsigned pm_operation_index(pm_operation op){return pm_operation_valid(op)?(unsigned)op.value-1:UINT_MAX;}
static inline bool pm_operation_from_index(unsigned index,pm_operation *out){
    if(index>=4||!out)return false;
    *out=(pm_operation){(uint8_t)(index+1)};return true;
}
static inline const char *pm_operation_name(pm_operation op){
    static const char *const names[]={"SUB","UP","POWER","DOWN"};
    return pm_operation_valid(op)?names[pm_operation_index(op)]:"INVALID";
}
static inline bool pm_operation_from_name(const char *name,pm_operation *out){
    if(!name||!out)return false;
    for(unsigned i=0;i<4;i++){pm_operation op={(uint8_t)(i+1)};
        if(!strcmp(name,pm_operation_name(op))){if(out)*out=op;return out!=NULL;}}
    return false;
}
typedef enum { PM_INPUT_INVALID, PM_INPUT_UNSUPPORTED, PM_INPUT_BUSY, PM_INPUT_IGNORED,
               PM_INPUT_READY, PM_INPUT_DISPATCHED, PM_INPUT_FAILED } pm_input_result;
static inline const char *pm_input_result_name(pm_input_result r){
    static const char *const names[]={"invalid","unsupported","busy","ignored","ready","dispatched","failed"};
    return (unsigned)r<sizeof names/sizeof names[0]?names[r]:"invalid";
}
typedef pm_input_result (*pm_input_handler)(pm_operation operation);
