#pragma once
#include <stdbool.h>
#include <stdint.h>

/* Flash identities are not menu positions. Never renumber existing records. */
typedef enum { PM_PROFILE_HOME=0, PM_PROFILE_AWAY=1, PM_PROFILE_HOME2=2,
               PM_PROFILE_COUNT=3 } pm_profile_id;
static inline const char *pm_profile_name(unsigned id){
    switch(id){
    case PM_PROFILE_HOME:return "HOME";
    case PM_PROFILE_HOME2:return "HOME2";
    case PM_PROFILE_AWAY:return "AWAY";
    default:return "UNKNOWN";
    }
}
/* Query -> pure bounded selection; the caller owns config IO and persistence. */
static inline uint8_t pm_profile_step(uint8_t current,uint8_t ready,bool forward){
    const uint8_t order[]={PM_PROFILE_HOME,PM_PROFILE_HOME2,PM_PROFILE_AWAY};
    unsigned pos=0;
    for(unsigned i=0;i<PM_PROFILE_COUNT;i++)if(order[i]==current){pos=i;break;}
    for(unsigned n=1;n<=PM_PROFILE_COUNT;n++){
        unsigned at=(pos+(forward?n:PM_PROFILE_COUNT-n))%PM_PROFILE_COUNT;
        if(ready&(1u<<order[at]))return order[at];
    }
    return current;
}
static inline uint32_t pm_profile_restore(uint32_t saved,bool home2_ready){
    return saved==PM_PROFILE_HOME2&&!home2_ready?PM_PROFILE_HOME:saved;
}
