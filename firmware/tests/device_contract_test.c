#include <assert.h>
#include <stdio.h>
#include "input_rules.h"
#include "device_api.h"
static pm_api_result extension(uint8_t kind,const char *body){
    assert(kind==201 && body[0]=='x');return PM_API_PENDING;
}
/* Semantic rules are independent of GPIO count/gesture interpretation. The
 * physical 0/2/4 and arbitrary-layout gesture tests characterize that adapter. */
static pm_input_action expected(app_state state,unsigned key){
    if(state==STANDBY)return key==2?PM_INPUT_WAKE:PM_INPUT_IGNORE;
    if(state==MENU){
        if(key==1||key==3)return PM_INPUT_MENU_MOVE;
        return key==0?PM_INPUT_MENU_ACCEPT:key==2?PM_INPUT_WAKE:PM_INPUT_IGNORE;
    }
    if(state!=ACTIVE)return key==0?PM_INPUT_MENU:key==2?PM_INPUT_STANDBY:PM_INPUT_IGNORE;
    return key==2?PM_INPUT_POWER:key==0?PM_INPUT_SUB:key==1?PM_INPUT_UP:PM_INPUT_DOWN;
}
int main(void){
    unsigned n=0;
    for(unsigned state=0;state<8;state++)for(unsigned mask=0;mask<=PM_OPERATION_MASK;mask++)
    for(unsigned sub=0;sub<2;sub++)for(unsigned key=0;key<6;key++)for(unsigned blocked=0;blocked<2;blocked++){
        pm_input_view before={(app_state)state,sub,(uint8_t)mask,blocked};pm_operation operation={0};
        bool valid=pm_operation_from_index(key,&operation);pm_input_plan p=pm_input_decide(before,operation);
        pm_input_result result=!valid?PM_INPUT_INVALID:!(mask&(1u<<key))?PM_INPUT_UNSUPPORTED:blocked?PM_INPUT_BUSY:expected(before.state,key)==PM_INPUT_IGNORE?PM_INPUT_IGNORED:PM_INPUT_READY;
        assert(p.result==result);
        assert(p.transition.action==(result==PM_INPUT_READY?expected(before.state,key):PM_INPUT_IGNORE));
        assert(p.transition.clear_retained==(result==PM_INPUT_READY&&state==ACTIVE));
        assert(p.transition.await_before_send==(result==PM_INPUT_READY&&state==ACTIVE&&!sub&&(key==1||key==3)));
        assert(before.state==(app_state)state&&before.sub_mode==sub);n++;
    }
    pm_operation valid={PM_OP_UP};
    assert(pm_input_decide((pm_input_view){(app_state)255,0,PM_OPERATION_MASK,0},valid).result==PM_INPUT_INVALID);
    assert(pm_input_decide((pm_input_view){ACTIVE,0,255,0},valid).result==PM_INPUT_INVALID);
    const pm_device_api apis[]={{201,"extension",extension}};
    assert(!pm_device_api_find(apis,1,200));
    const pm_device_api *a=pm_device_api_find(apis,1,201);
    assert(a && a->handle(201,"x")==PM_API_PENDING);
    printf("PASS %u local input cases; extension API requires no router change\n",n);
}
