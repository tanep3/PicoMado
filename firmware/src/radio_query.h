#pragma once
#include <stdbool.h>
#include <stdint.h>
/* Range-checked API payload only, NOT CDC-verified radio configuration.
 * Zero initialization is invalid, distinct from the candidate value NONE. */
typedef struct { uint8_t encoded; } pm_interference_candidate;
static inline bool pm_interference_candidate_from_raw(uint32_t raw,pm_interference_candidate *out){
    if(!out||raw>4)return false;
    *out=(pm_interference_candidate){(uint8_t)(raw+1)};return true;
}
static inline bool pm_interference_candidate_value(pm_interference_candidate mode,uint32_t *out){
    if(!out||mode.encoded<1||mode.encoded>5)return false;
    *out=(uint32_t)mode.encoded-1;return true;
}
typedef enum { PM_RADIO_UNAVAILABLE,PM_RADIO_CONTEXT,PM_RADIO_BUSY,PM_RADIO_SDK_ERROR,
               PM_RADIO_UNKNOWN,PM_RADIO_TRANSPORT_RETURNED } pm_radio_status;
typedef struct { pm_radio_status status;int sdk_result;uint32_t raw; } pm_radio_read;
static inline pm_radio_read pm_radio_rejected(pm_radio_status status){
    return (pm_radio_read){status,0,UINT32_MAX};
}
static inline pm_radio_read pm_radio_decode(int result,uint32_t raw){
    pm_interference_candidate mode;
    return (pm_radio_read){result?PM_RADIO_SDK_ERROR:
        (pm_interference_candidate_from_raw(raw,&mode)?PM_RADIO_TRANSPORT_RETURNED:PM_RADIO_UNKNOWN),result,raw};
}
static inline bool pm_radio_transport_candidate(pm_radio_read read,pm_interference_candidate *out){
    return read.status==PM_RADIO_TRANSPORT_RETURNED&&!read.sdk_result&&pm_interference_candidate_from_raw(read.raw,out);
}
static inline const char *pm_radio_status_name(pm_radio_status status){
    static const char *const names[]={"unavailable","context","busy","sdk_error","unknown","transport_returned"};
    return (unsigned)status<sizeof names/sizeof names[0]?names[status]:"unknown";
}
typedef pm_radio_read (*pm_radio_reader)(void);
