// The pre-fix header compiles this test but rejects the real raw=0x84 case.
#include <assert.h>
#include <stdint.h>
#include "radio_query.h"
int main(void){
 pm_radio_read r=pm_radio_decode(0,UINT32_C(0x84));
 pm_interference_candidate setting;uint32_t value;
 assert(r.status==PM_RADIO_TRANSPORT_RETURNED);
 assert(pm_radio_transport_candidate(r,&setting));
 assert(pm_interference_candidate_value(setting,&value)&&value==4);
 return 0;
}
