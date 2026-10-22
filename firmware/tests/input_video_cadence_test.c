// Real main GPIO/video/serializer paths; work cadence and GPIO are models.
#define PM_CAUSAL_FIXTURE 1
#define PM_CONTROL_TEST_ENTRY pm_cadence_existing_control_cases
#include "control_reply_test.c"
static unsigned cadence_kinds[32];
static void cadence_wire(uint8_t kind,const char *body){assert(body&&kind<32);cadence_kinds[kind]++;}
static void cadence(unsigned step,unsigned key){
    setup();memset(buttons,0,sizeof buttons);memset(cadence_kinds,0,sizeof cadence_kinds);
    clock_ms=0;pressed=-1;usb_connected=false;checking_frame=false;wire_observer=cadence_wire;
    pm_usb_control_init(1,PM_BUTTON_COUNT);pm_usb_control_set_operations(PM_INPUT_OPERATIONS_MASK,usb_operation);
    static uint8_t pixels[PM_IMAGE_BYTES];unsigned sequence=0;
    const unsigned kind[]={14,15,7,16};
    pm_token context={1,1,1,42,100};
    pm_video_prepare(&video,context,25,1,0);
    unsigned rendered_before_input=0;
    unsigned phase=0;
    for(unsigned at=0;at<1200;at+=step?step:((phase++%21)==0?40u:1u)){
        clock_ms=at;pressed=at>=100&&at<500?(int)key:-1;
        poll_local_inputs(at,true);
        if(cadence_kinds[kind[key]]){rendered_before_input=(unsigned)video.rendered;break;}
        context.frame_id=100+sequence;
        assert(pm_video_offer(&video,context,sequence++,pixels,sizeof pixels));
        poll_video();
        // This supplied cadence represents all main work (decode/draw/IO),
        // not a measured SPI duration. GPIO is still sampled once per loop.
    }
    printf("cadence_ms=%u key=%u notifications=%u rendered_before_input=%u rendered_total=%llu\n",
        step,key,cadence_kinds[kind[key]],rendered_before_input,(unsigned long long)video.rendered);
    fflush(stdout);
    assert(cadence_kinds[kind[key]]==1&&rendered_before_input>0);
    assert(input_sends==1);
}
int main(int argc,char **argv){
    assert(PM_BUTTON_COUNT==4);
    if(argc==3){unsigned step=(unsigned)strtoul(argv[1],NULL,10),key=(unsigned)strtoul(argv[2],NULL,10);assert(key<4);cadence(step,key);return 0;}
    const unsigned steps[]={5,20,35,36,40,50,100,0};
    for(unsigned c=0;c<sizeof steps/sizeof steps[0];c++)for(unsigned key=0;key<4;key++)cadence(steps[c],key);
    puts("PASS video cadence: all4 direct GPIO actions at5..100ms and40ms+20x1ms; render advances and expected wire kind arrives once");
    return 0;
}
