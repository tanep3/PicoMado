#include <assert.h>
#include <stdio.h>
#include "core.h"
static int step(pm_button *b,unsigned i,bool pressed,uint32_t now,uint32_t threshold,bool standby){
    return pm_button_input_step(b,2,i,pressed,now,threshold,standby);
}
int main(void){
    pm_button b={0};
    assert(step(&b,0,true,0,1800,false)==-1);
    assert(step(&b,0,true,35,1800,false)==-1);
    assert(step(&b,0,false,100,1800,false)==-1);
    assert(step(&b,0,false,135,1800,false)==1);
    assert(step(&b,0,false,200,1800,false)==-1);
    b=(pm_button){0};
    assert(step(&b,0,true,300,1800,false)==-1);
    assert(step(&b,0,true,335,1800,false)==-1);
    // Settings arriving mid-press affect the next press only.
    assert(step(&b,0,true,1835,1500,false)==-1);
    assert(step(&b,0,true,2135,1500,false)==0);
    assert(step(&b,0,true,3000,1500,false)==-1);
    assert(step(&b,0,false,3100,1500,false)==-1);
    assert(step(&b,0,false,3135,1500,false)==-1);
    for(unsigned i=0;i<2;i++){
        b=(pm_button){0};
        assert(step(&b,i,true,0,1800,true)==-1);
        assert(step(&b,i,true,35,1800,true)==-1);
        assert(step(&b,i,true,1834,1800,true)==-1);
        assert(step(&b,i,true,1835,1800,true)==(i==1?2:-1));
        assert(step(&b,i,true,1840,1800,false)==-1);
        assert(step(&b,i,false,1850,1800,false)==-1);
        assert(step(&b,i,false,1885,1800,false)==-1);
    }
    b=(pm_button){0};
    assert(step(&b,1,true,0xfffffff0u,1500,false)==-1);
    assert(step(&b,1,true,19,1500,false)==-1);
    assert(step(&b,1,true,1519,1500,false)==2);
    b=(pm_button){0};
    assert(pm_button_input_step(&b,4,1,true,0,1800,false)==-1);
    assert(pm_button_input_step(&b,4,1,true,35,1800,false)==1);
    b=(pm_button){0};
    assert(pm_button_input_step(&b,0,0,true,35,1800,false)==-1);
    puts("input: 2-button short/long/no repeat/no short after long/next-press threshold/standby/wrap; 4/0 inputs PASS");
}
