// Real GPIO adapter + operation handler; network/hardware use existing mocks.
#define PM_CAUSAL_FIXTURE 1
#define PM_CONTROL_TEST_ENTRY pm_sampling_existing_control_cases
#include "control_reply_test.c"
static unsigned sampling_kinds[32];
static void sampling_wire(uint8_t kind,const char *body){assert(body&&kind<32);sampling_kinds[kind]++;}
static void sampling_setup(void){
    setup();memset(buttons,0,sizeof buttons);memset(sampling_kinds,0,sizeof sampling_kinds);
    wire_observer=sampling_wire;pressed=-1;usb_connected=false;clock_ms=0;
    pm_usb_control_init(1,PM_BUTTON_COUNT);pm_usb_control_set_operations(PM_INPUT_OPERATIONS_MASK,usb_operation);
}
static void sampling_tick(uint32_t at,int key){clock_ms=at;pressed=key;poll_local_inputs(at,true);}
static void sampling_range(uint32_t first,uint32_t last,int key){
    for(uint32_t at=first;at<=last;at+=5)sampling_tick(at,key);
}
static void counterexample(unsigned which){
    static const unsigned thresholds[]={1500,1800,2000};
    unsigned key=which/6,threshold=thresholds[(which%6)/2];
    bool release_once=which%2;sampling_setup();settings.long_ms=threshold;
    sampling_range(0,40,(int)key);
    if(release_once)sampling_tick(100,-1);
    // Physical timeline: down[0,100), up[100,threshold+80), down100ms.
    // No input calls are made in the blind interval.
    sampling_tick(threshold+120,(int)key);
    assert(!sampling_kinds[14]&&!sampling_kinds[7]&&!sampling_kinds[15]&&!sampling_kinds[16]);
    sampling_range(threshold+180,threshold+220,-1);
    uint32_t fresh=threshold+230;
    sampling_range(fresh,fresh+100,(int)key);sampling_range(fresh+105,fresh+145,-1);
    assert(sampling_kinds[key?16:15]==1&&!sampling_kinds[14]&&!sampling_kinds[7]);
    // A new continuous hold, after a verified release, still fires once.
    fresh+=160;sampling_range(fresh,fresh+threshold+100,(int)key);
    sampling_range(fresh+threshold+105,fresh+threshold+145,-1);
    assert(sampling_kinds[key?7:14]==1&&sampling_kinds[key?16:15]==1);
    printf("PASS sampled real main case=%u key=%u threshold=%u release_once=%u cancel/rearm/long\n",which,key,threshold,release_once);
}
int main(int argc,char **argv){
    assert(PM_BUTTON_COUNT==2);
    if(argc==2){unsigned which=(unsigned)strtoul(argv[1],NULL,10);assert(which<12);counterexample(which);return 0;}
    for(unsigned which=0;which<12;which++)counterexample(which);
    sampling_setup();sampling_tick(0,-1);pm_button saved=buttons[0];
    assert(usb_operation((pm_operation){PM_OP_UP})==PM_INPUT_DISPATCHED);
    assert(usb_operation((pm_operation){PM_OP_SUB})==PM_INPUT_DISPATCHED);
    assert(!memcmp(&saved,&buttons[0],sizeof saved));
    // A pre-display timestamp cannot backdate a newly observed physical press.
    sampling_setup();clock_ms=2000;pressed=0;poll_local_inputs(0,true);
    sampling_range(2005,2100,0);sampling_range(2105,2145,-1);
    assert(sampling_kinds[15]==1&&!sampling_kinds[14]);
    puts("PASS sampled real main: USB leaves GPIO state intact; fresh sample clock");
    return 0;
}
