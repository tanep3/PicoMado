#include <assert.h>
#include <stdio.h>
#include <string.h>
#include "core.h"
static int observe(pm_button *button,unsigned key,bool down,uint32_t at,unsigned long_ms,bool standby){
    pm_operation op;bool held;
    return pm_button_sampled_binding_step(button,pm_button_binding_at(2,key),down,at,long_ms,standby,&op,&held)?(int)pm_operation_index(op):-1;
}
static void stream(pm_button *b,unsigned key,bool down,uint32_t start,unsigned duration,unsigned threshold,int expected){
    unsigned events=0;
    for(unsigned dt=0;dt<=duration;dt+=5){int op=observe(b,key,down,start+dt,threshold,false);if(op>=0){assert(op==expected);events++;}}
    assert(events==(expected>=0?1u:0u));
}
int main(void){
    pm_button b={0};
    assert(observe(&b,0,true,0,1500,false)==-1);
    assert(observe(&b,0,true,35,1500,false)==-1); // exactly35 admitted
    assert(observe(&b,0,true,71,1500,false)==-1); //36 cancels
    stream(&b,0,true,76,2000,1500,-1); // no delayed long while still held
    stream(&b,0,false,2081,40,1500,-1); // no short on cancelled release
    stream(&b,0,true,2126,100,1500,-1);stream(&b,0,false,2231,40,1500,1);
    // A second gap while verifying release restarts its35ms qualification.
    b=(pm_button){0};observe(&b,0,true,0,1800,false);observe(&b,0,true,35,1800,false);
    assert(observe(&b,0,false,100,1800,false)==-1);assert(observe(&b,0,false,140,1800,false)==-1);
    assert(observe(&b,0,true,170,1800,false)==-1);stream(&b,0,true,175,2000,1800,-1);
    stream(&b,0,false,2180,40,1800,-1);stream(&b,0,true,2225,100,1800,-1);stream(&b,0,false,2330,40,1800,1);
    // Ordinary wrap with continuous samples is not a gap.
    b=(pm_button){0};uint32_t start=UINT32_MAX-100;
    stream(&b,0,true,start,1640,1500,0);stream(&b,0,false,start+1645,40,1500,-1);
    // Independent simultaneous buttons retain their own threshold snapshots.
    pm_button pair[2]={{0}};unsigned got[4]={0};
    for(unsigned at=0;at<=2000;at+=5)for(unsigned key=0;key<2;key++){
        bool down=key?at<1900:at<100;int op=observe(&pair[key],key,down,at,1800,false);if(op>=0)got[op]++;
    }
    assert(got[1]==1&&got[2]==1&&!got[0]&&!got[3]);
    // Normal contact bounce: short pulses do not become stable events.
    b=(pm_button){0};assert(observe(&b,0,true,0,1800,false)==-1);assert(observe(&b,0,false,5,1800,false)==-1);
    stream(&b,0,false,10,50,1800,-1);stream(&b,0,true,65,100,1800,-1);stream(&b,0,false,170,40,1800,1);
    // Changing configuration while held only affects a following press.
    b=(pm_button){0};unsigned events=0;
    for(unsigned at=0;at<=1900;at+=5){
        int op=observe(&b,0,true,at,at<500?1800:1500,false);
        if(op>=0){assert(at==1835&&op==0);events++;}
    }
    assert(events==1);
    for(unsigned key=0;key<2;key++){
        b=(pm_button){0};events=0;
        for(unsigned at=0;at<=1900;at+=5){int op=observe(&b,key,true,at,1800,true);if(op>=0){assert(key==1&&op==2);events++;}}
        assert(events==(key==1?1u:0u));
    }
    // Direct operations do not derive meaning from elapsed hold time. Slow
    // video cadence must not leave every key permanently release-required.
    const unsigned cadences[]={5,20,35,36,40,50,100};
    for(unsigned c=0;c<sizeof cadences/sizeof cadences[0];c++)for(unsigned key=0;key<4;key++){
        b=(pm_button){0};events=0;
        for(unsigned at=0;at<1200;at+=cadences[c]){
            pm_operation actual;bool held;
            if(pm_button_sampled_binding_step(&b,pm_button_binding_at(4,key),at>=100&&at<500,at,1800,false,&actual,&held)){
                assert(pm_operation_index(actual)==key&&!held);events++;
            }
        }
        assert(events==1&&!b.release_required);
    }
    // Invalid input cannot mutate the sampler or gesture.
    pm_button before=b;pm_operation op;bool held;
    assert(!pm_button_sampled_binding_step(&b,NULL,true,9999,1800,false,&op,&held));assert(!memcmp(&before,&b,sizeof b));
    puts("PASS physical sample continuity:35/36ms,cancel/release/rearm,multiple gaps,wrap,simultaneous,bounce,invalid");
}
