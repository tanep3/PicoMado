#pragma once
#include <stddef.h>
#include <stdint.h>

/* Device API contracts. Routing is a pure lookup; handlers own device IO.
 * A transport adapter validates the envelope before invoking one handler.
 * Wire kind/type names remain the existing protocol's names. */
typedef enum {
    PM_API_REJECTED, PM_API_IGNORED, PM_API_PENDING,
    PM_API_COMPLETED, PM_API_ERROR, PM_API_CANCELLED
} pm_api_result;
typedef pm_api_result (*pm_api_handler)(uint8_t kind,const char *body);
typedef struct {uint8_t kind;const char *type;pm_api_handler handle;} pm_device_api;
static inline const pm_device_api *pm_device_api_find(
    const pm_device_api *apis,size_t count,uint8_t kind){
    for(size_t i=0;i<count;i++)if(apis[i].kind==kind)return &apis[i];
    return NULL;
}
