/* Production prepare/tick/send implementation with fake SDK. PCM regression
 * runs first; SBC requires bit-identical frames and zero encoder invocations. */
#define main pcm_runtime_main
#include "bluetooth_rebuffer_runtime_test.c"
#undef main
static uint8_t golden[119];
static size_t golden_size;
static void load_golden(const char *path,unsigned mode,unsigned allocation,unsigned bitpool){
 FILE *f=fopen(path,"r");assert(f);char line[1024],hex[240];unsigned m,a,b;bool found=false;
 while(fgets(line,sizeof line,f))if(sscanf(line,"%u %u %u %239s",&m,&a,&b,hex)==4&&m==mode&&a==allocation&&b==bitpool){
  golden_size=strlen(hex)/2;
  for(size_t i=0;i<golden_size;i++){unsigned v;assert(sscanf(hex+2*i,"%2x",&v)==1);golden[i]=(uint8_t)v;}
  found=true;break;
 }
 fclose(f);assert(found);
}
static void sbc_begin(unsigned mode,unsigned allocation,unsigned bitpool,uint64_t first){
 media_stop();wanted=true;generation=7;codec_epoch=12;codec_config=(pm_bt_sbc_config){48000,2,16,8,mode,allocation,bitpool};
 now=100;streaming=false;rate=48000;encoder=&encoder_value;
 sends=requests=pauses=starts=underflows=errors=encodes=0;encoded_frames=0;synchronous=true;send_status=0;
 timestamp=deadline=0;samples_sent=0;clock_started=false;
 pm_bt_sbc_config got;uint64_t epoch;
 assert(pm_bt_runtime_audio_sbc_begin(9,first,&got,&epoch));assert(pm_bt_sbc_config_equal(&got,&codec_config)&&epoch==12);
}
static void sbc_fill(size_t used){
 uint8_t chunk[8*119];for(unsigned i=0;i<8;i++)memcpy(chunk+i*golden_size,golden,golden_size);
 while(audio.used<used){size_t n=(used-audio.used)/128;if(n>8)n=8;assert(n);
  assert(pm_bt_runtime_audio_sbc_push(9,accepted_frames,chunk,n*golden_size)==PM_BT_PCM_OK);
 }
}
static void assert_packet(void){
 assert(!encodes&&encoded[0]==diagnostics.last_count);
 for(unsigned i=0;i<encoded[0];i++)assert(!memcmp(encoded+1+i*golden_size,golden,golden_size));
 assert(diagnostics.last_bytes==1+encoded[0]*golden_size);
}
static void run_tuple(const char *path,unsigned mode,unsigned alloc,unsigned pool){
 load_golden(path,mode,alloc,pool);sbc_begin(mode,alloc,pool,13);sbc_fill(16384);
 assert(pm_bt_runtime_audio_capacity()==16384&&pm_bt_runtime_audio_free_frames()==0);
 assert(!pm_bt_runtime_play());streaming=true;synchronous=false;tick(&timer);
 assert(pending&&prepared_count&&!sends&&consumed_frames==13&&pm_bt_runtime_audio_free_frames()==0);
 assert(pm_bt_runtime_audio_sbc_push(9,accepted_frames,golden,golden_size)==PM_BT_PCM_FULL);
 uint64_t prepared=prepared_count*128;send_audio();assert(sends==1&&consumed_frames==13+prepared);assert_packet();
 assert(pm_bt_runtime_audio_free_frames()==prepared);
 sbc_fill(16384);synchronous=true;
 // Drain without refill until a real bounded rebuffer; RTP positions never jump.
 unsigned guard=0;
 while(!rebuffer.buffering&&audio.active){assert(++guard<200);at(now+5);}
 assert(audio.active&&rebuffer.buffering&&!underflows&&!encodes);
 uint64_t before=consumed_frames;sbc_fill(16384);at(now+5);assert(consumed_frames>before&&!rebuffer.buffering);assert_packet();
 // A changed negotiated codec invalidates prepared work before SDK send.
 synchronous=false;at(now+50);assert(pending);unsigned sent=sends;codec_epoch++;send_audio();assert(!audio.active&&sends==sent&&!prepared_count);
 // Config change during ingress cannot admit bytes, or prepare a future packet.
 sbc_begin(mode,alloc,pool,13);codec_epoch++;assert(pm_bt_runtime_audio_sbc_push(9,13,golden,golden_size)==PM_BT_PCM_INACTIVE);assert(accepted_frames==13);
 sbc_begin(mode,alloc,pool,13);sbc_fill(16384);streaming=true;codec_config.bitpool=codec_config.bitpool==53?32:53;prepare_audio();assert(!audio.active&&!prepared_count&&!sends);
 // Finite one-frame EOF from an unaligned requested seek, exact unchanged bytes.
 sbc_begin(mode,alloc,pool,19);assert(pm_bt_runtime_audio_finish(9,147));sbc_fill(128);assert(!pm_bt_runtime_play());streaming=true;tick(&timer);
 assert(consumed_frames==147&&sends==1&&encoded[0]==1);assert_packet();at(last_packet_end_ms);assert(!audio.active);
 // Timed-out refill leaves counters and no late callback resurrection.
 sbc_begin(mode,alloc,pool,0);sbc_fill(16384);streaming=true;tick(&timer);guard=0;
 while(!rebuffer.buffering){assert(++guard<200);at(now+5);}
 at(rebuffer.since+1500);assert(!audio.active&&underflows==1);sent=sends;send_audio();assert(sends==sent);
}
int main(int argc,char **argv){
 assert(argc==2);pcm_runtime_main();
 for(unsigned mode=2;mode<=3;mode++)for(unsigned a=0;a<=1;a++)for(unsigned p=0;p<3;p++)run_tuple(argv[1],mode,a,(unsigned[]){2,32,53}[p]);
 puts("SBC production runtime: 12 host fixtures, zero encode, RTP aggregation, prepared credit, rebuffer, epoch cancellation and EOF PASS");
 return 0;
}
