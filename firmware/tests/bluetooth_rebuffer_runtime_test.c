/* Compile the production PCM adapter. Only SDK codec/radio/timer are doubles. */
#include <assert.h>
#include <stdio.h>
#include <string.h>
#include "bluetooth_runtime.h"
#include "bluetooth_packet_policy.h"
#include "bluetooth_rebuffer.h"
#define SBC_CHANNEL_MODE_JOINT_STEREO 1
#define ERROR_CODE_COMMAND_DISALLOWED 12
static uint64_t generation=7,codec_epoch=1;
static pm_bt_sbc_config codec_config={48000,2,16,8,3,0,32};
static bool wanted=true,streaming,pending,input_enabled,clock_started,pcm_eof,timer_scheduled;
static uint32_t rate=48000,timestamp,deadline,can_requested_ms,timer_due_ms,input_epoch,last_packet_end_ms;
static uint64_t accepted_frames,consumed_frames,samples_sent,pcm_end,pcm_first;
static uint8_t encoded[1000],seid=1;
static uint16_t cid=3;
static pm_bluetooth_audio audio;
static unsigned prepared_count,prepared_bytes;
static pm_bt_audio_diagnostics diagnostics;
static pm_bt_rebuffer rebuffer;
typedef struct {int n;} btstack_timer_source_t;
static btstack_timer_source_t timer;
static struct {struct {unsigned s16BitPool,s16ChannelMode;} params;} encoder_state={{32,1}};
static uint32_t now,next_timer;
static bool timer_registered;
static unsigned sends,requests,pauses,starts,underflows,errors,encodes;
static uint64_t encoded_frames;
static uint32_t sent_times[128],sent_timestamps[128];
static bool synchronous=true;
static uint8_t send_status;
static pm_bt_event last_event;
static unsigned frames(void *p){(void)p;return 128;}
static uint8_t encode(void *s,const int16_t *pcm,uint8_t *out){
 (void)s;assert(pcm[0]==(int16_t)(encoded_frames&0x7fff));encoded_frames+=128;encodes++;memset(out,0,77);return 0;
}
static unsigned bytes(void *p){(void)p;return 77;}
static const struct {unsigned (*num_audio_frames)(void *);uint8_t (*encode_signed_16)(void *,const int16_t *,uint8_t *);unsigned (*sbc_buffer_length)(void *);} encoder_value={frames,encode,bytes},*encoder=&encoder_value;
static uint32_t btstack_run_loop_get_time_ms(void){return now;}
static int a2dp_max_media_payload_size(unsigned c,unsigned s){assert(c==cid&&s==seid);return 704;}
static void emit(pm_bt_event e){last_event=e;if(e.kind==PM_BT_UNDERFLOW)underflows++;else if(e.kind==PM_BT_ERROR)errors++;else assert(0);}
static void btstack_run_loop_remove_timer(btstack_timer_source_t *t){assert(t==&timer);next_timer=0;timer_registered=false;}
static void btstack_run_loop_set_timer(btstack_timer_source_t *t,uint32_t ms){assert(t==&timer);next_timer=now+ms;}
static void btstack_run_loop_add_timer(btstack_timer_source_t *t){assert(t==&timer&&!timer_registered);timer_registered=true;}
static uint8_t a2dp_source_pause_stream(unsigned c,unsigned s){assert(c==cid&&s==seid);pauses++;return 0;}
static uint8_t a2dp_source_start_stream(unsigned c,unsigned s){assert(c==cid&&s==seid);starts++;return 0;}
static uint8_t a2dp_source_stream_send_media_payload_rtp(unsigned c,unsigned s,int marker,uint32_t ts,const uint8_t *data,unsigned length){
 assert(c==cid&&s==seid&&!marker&&data==encoded&&length==1u+(audio.sbc?pm_bt_sbc_frame_bytes(&audio.sbc_config):77u)*data[0]);
 if(send_status)return send_status;
 assert(sends<128);sent_times[sends]=now;sent_timestamps[sends]=ts;sends++;return 0;
}
static void send_audio(void);
static void prepare_audio(void);
static void a2dp_source_stream_endpoint_request_can_send_now(unsigned c,unsigned s){assert(c==cid&&s==seid);requests++;if(synchronous)send_audio();}
#include "../src/bluetooth_audio_runtime.inc"
static void fill_to(unsigned available){
 int16_t pcm[512];
 while(audio.used<available){
  unsigned n=available-(unsigned)audio.used;if(n>256)n=256;
  for(unsigned i=0;i<n;i++)pcm[2*i]=pcm[2*i+1]=(int16_t)((accepted_frames+i)&0x7fff);
  assert(pm_bt_runtime_audio_push(audio.generation,accepted_frames,pcm,n)==PM_BT_PCM_OK);
 }
}
static void reset(void){
 media_stop();wanted=true;generation=7;now=100;streaming=false;rate=48000;encoder=&encoder_value;
 sends=requests=pauses=starts=underflows=errors=encodes=0;encoded_frames=0;synchronous=true;send_status=0;
 timestamp=deadline=0;samples_sent=0;clock_started=false;
 assert(pm_bt_runtime_audio_begin(9,0));fill_to(PM_BT_PCM_FRAMES);assert(!pm_bt_runtime_play());
 streaming=true;tick(&timer);assert(sends==1&&consumed_frames==1152&&timestamp==1152);
}
static void at(uint32_t t){now=t;timer_registered=false;tick(&timer);}
static void three_packets(void){at(101);assert(sends==1);at(124);at(125);at(148);assert(sends==3&&consumed_frames==3456&&audio.used==640);}
static void test_refill_and_clock(void){
 reset();three_packets();at(149);assert(rebuffer.shortage&&!rebuffer.buffering&&audio.active);
 at(171);assert(audio.active&&!rebuffer.buffering);at(173);assert(rebuffer.buffering&&diagnostics.rebuffer_count==1);
 at(199);assert(audio.active&&!underflows&&!pauses&&consumed_frames==3456);
 fill_to(1152);at(200);assert(sends==3); // a full packet alone cannot exit rebuffer
 fill_to(PM_BT_PCM_FRAMES);at(350);assert(sends==4&&!rebuffer.buffering&&!underflows&&!pauses);
 assert(sent_timestamps[3]==3456&&timestamp==4608&&consumed_frames==4608);
 assert(sent_times[3]==350&&deadline==278&&diagnostics.rebuffer_total_ms==201);
 at(351);assert(sends==4&&next_timer==374);at(374);assert(sends==5&&sent_timestamps[4]==4608);
 assert(sent_times[4]-sent_times[3]==24); // unchanged sample rate, no catch-up burst
}
static void test_timely_and_eof(void){
 reset();three_packets();at(149);fill_to(1152);at(160);assert(!rebuffer.shortage&&!rebuffer.buffering&&sends==3);at(172);assert(sends==4&&!diagnostics.rebuffer_count);
 reset();three_packets();at(149);at(173);assert(rebuffer.buffering);
 assert(pm_bt_runtime_audio_finish(9,accepted_frames));at(190);assert(sends==4&&consumed_frames==4096&&encoded[0]==5);
 at(191);assert(audio.active&&next_timer==204);at(203);assert(audio.active);at(204);assert(!audio.active&&!underflows); // preserve the final packet playback interval
}
static void test_deadline_and_cancel(void){
 reset();three_packets();at(149);at(173);at(1648);assert(audio.active);at(1649);assert(!audio.active&&underflows==1&&last_event.status==0xff04&&last_event.detail2==1500);
 unsigned prior=sends;send_audio();at(1650);assert(sends==prior&&underflows==1);
 // Refilled but callback never grants: deadline still ends the episode.
 reset();three_packets();at(149);at(173);fill_to(PM_BT_PCM_FRAMES);synchronous=false;at(300);assert(pending&&rebuffer.resuming&&next_timer==305);
 at(1649);assert(!audio.active&&underflows==1);send_audio();assert(sends==3);
 for(unsigned phase=0;phase<3;phase++)for(unsigned cancel=0;cancel<3;cancel++){
  reset();three_packets();at(149);if(phase)at(173);
  if(phase==2){fill_to(PM_BT_PCM_FRAMES);synchronous=false;at(300);assert(pending);}
  if(cancel==0)pm_bt_runtime_audio_end();else if(cancel==1){generation++;at(301);}else {media_stop();wanted=false;}
  prior=sends;send_audio();at(400);assert(sends==prior&&!audio.active&&!prepared_count&&!rebuffer.stream);
 }
 // An asynchronous resumed grant replaces its watchdog timer, never double-adds.
 reset();three_packets();at(149);at(173);fill_to(PM_BT_PCM_FRAMES);synchronous=false;at(300);
 assert(timer_registered&&next_timer==305);now=302;send_audio();assert(sends==4&&timer_registered&&next_timer==303);
 // Nonpending callback cannot send a packet already prepared for a future deadline.
 reset();at(101);assert(prepared_count&&!pending);send_audio();assert(sends==1);
}
static void test_repeat_budget(void){
 reset();
 for(unsigned episode=0;episode<3;episode++){
  // Drain whatever complete packets remain, retaining the actual ordered PCM.
  while(audio.used>=1152||prepared_count){at(now+24);}
  uint32_t shortage=now+1;at(shortage);at(shortage+30);assert(rebuffer.buffering);
  fill_to(PM_BT_PCM_FRAMES);at(shortage+100);assert(audio.active&&!rebuffer.buffering);
 }
 while(audio.used>=1152||prepared_count){at(now+24);}
 at(now+1);at(now+30);assert(!audio.active&&underflows==1&&last_event.detail1==PM_BT_RB_EXHAUSTED);
}
int main(void){test_refill_and_clock();test_timely_and_eof();test_deadline_and_cancel();test_repeat_budget();puts("Bluetooth production rebuffer: credit/ring preservation, EOF, sample cadence, deadlines, cancellation and repeat budget PASS");return 0;}
