#include <assert.h>
#include <stdio.h>
#include "input_rules.h"
#include "device_api.h"
static pm_api_result extension(uint8_t kind,const char *body){
    assert(kind==201 && body[0]=='x');return PM_API_PENDING;
}
/* Characterize the existing local button rules across the complete view space. */
static pm_input_action original(app_state state,unsigned buttons,unsigned key,bool held){
    if(state==STANDBY)return key==2 && (buttons==4 || held)?PM_INPUT_WAKE:PM_INPUT_IGNORE;
    if(state==MENU){
        if(key==1 || key==3)return PM_INPUT_MENU_MOVE;
        if(key==0)return PM_INPUT_MENU_ACCEPT;
        return key==2?PM_INPUT_WAKE:PM_INPUT_IGNORE;
    }
    if(state!=ACTIVE)return key==0?PM_INPUT_MENU:key==2?PM_INPUT_STANDBY:PM_INPUT_IGNORE;
    return key==2?PM_INPUT_POWER:key==0?PM_INPUT_SUB:key==1?PM_INPUT_UP:key==3?PM_INPUT_DOWN:PM_INPUT_IGNORE;
}
int main(void){
    unsigned n=0;
    for(unsigned state=0;state<8;state++)for(unsigned buttons=0;buttons<=4;buttons+=2)
    for(unsigned sub=0;sub<2;sub++)for(unsigned key=0;key<5;key++)for(unsigned held=0;held<2;held++){
        pm_input_view before={(app_state)state,buttons,sub};
        pm_input_transition t=pm_input_decide(before,key,held);
        assert(t.action==original(before.state,buttons,key,held));
        assert(t.clear_retained==(state==ACTIVE));
        assert(t.await_before_send==(state==ACTIVE && !sub && (key==1 || key==3)));
        assert(before.state==(app_state)state && before.sub_mode==sub);n++;
    }
    const pm_device_api apis[]={{201,"extension",extension}};
    assert(!pm_device_api_find(apis,1,200));
    const pm_device_api *a=pm_device_api_find(apis,1,201);
    assert(a && a->handle(201,"x")==PM_API_PENDING);
    printf("PASS %u local input cases; extension API requires no router change\n",n);
}
