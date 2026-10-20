// Real production parser with the established hardware/network mocks.
#ifndef PM_CONTROL_TEST_ENTRY
#define PM_CONTROL_TEST_ENTRY main
#endif
#define PM_APP_TEST_ENTRY pm_fixture_entry
#include "app_timing_test.c"
#if PM_DISPLAY_TYPE == PM_DISPLAY_EINK
static bool panel_result;
bool pm_display_clock_frame(uint8_t *p,size_t n,unsigned m,uint64_t minute,bool reset){(void)m;(void)minute;(void)reset;return pm_display_frame(p,n);}
void pm_display_poll(void){}
bool pm_display_busy(void){return false;}
bool pm_display_take_result(bool *ok){if(!panel_result)return false;panel_result=false;*ok=true;return true;}
#endif
static unsigned serial;
static void setup(void){
 net_send_rejected=display_frame_rejected=false;
 defaults();state=ACTIVE;network_stage=PM_NET_READY;wifi_enabled=false;
 incarnation=session_epoch=display_epoch=1;owner_epoch=42;sub_mode=false;
 pending_standby=pending_sub_cancel=pending_render_ack=have_display=await_selection=false;
 checking_frame=true;frame_receipts=frame_renders=0;pm_reset(&assembly);
#if PM_DISPLAY_TYPE == PM_DISPLAY_LCD && (!defined(PM_TEST_OLD_FIRMWARE) || !PM_TEST_OLD_FIRMWARE)
 pm_video_reset(&video);receiving_stream=false;
#endif
#if PM_DISPLAY_TYPE == PM_DISPLAY_EINK
 drawing=panel_standby=false;panel_result=false;
#endif
}
static void token_json(char *out,size_t cap,unsigned id,unsigned epoch,unsigned session){
 snprintf(out,cap,"\"token\":{\"device_id\":\"%s\",\"incarnation\":1,\"session_epoch\":%u,\"display_epoch\":%u,\"screen_owner_epoch\":42,\"frame_id\":%u}",PM_DEVICE_ID,session,epoch,id);
}
static void start(unsigned id,unsigned epoch,unsigned session,const char *mode){
 uint8_t zeros[PM_IMAGE_BYTES]={0};char token[400];token_json(token,sizeof token,id,epoch,session);
 snprintf(json,sizeof json,"{\"type\":\"frame_start\",\"data\":{%s,\"mode\":\"%s\",\"width\":%u,\"height\":%u,\"format\":\"%s\",\"byte_len\":%u,\"crc32\":%lu}}",token,mode,PM_W,PM_H,PM_PIXEL_FORMAT_NAME,PM_IMAGE_BYTES,(unsigned long)pm_crc32(zeros,sizeof zeros));
 process(12,json);
}
static void chunk(unsigned id,unsigned epoch,unsigned session,unsigned offset,unsigned count){
 char token[400];token_json(token,sizeof token,id,epoch,session);
 size_t k=(size_t)snprintf(json,sizeof json,"{\"type\":\"frame_chunk\",\"data\":{%s,\"offset\":%u,\"bytes\":[",token,offset);
 for(unsigned i=0;i<count;i++){json[k++]='0';if(i+1<count)json[k++]=',';}
 memcpy(json+k,"]}}",4);process(13,json);
}
static void finish_image(unsigned id,unsigned epoch,unsigned session,unsigned offset){
 for(unsigned at=offset;at<PM_IMAGE_BYTES;at+=256){unsigned n=PM_IMAGE_BYTES-at;if(n>256)n=256;chunk(id,epoch,session,at,n);}
#if PM_DISPLAY_TYPE == PM_DISPLAY_EINK
 if(drawing){panel_result=true;poll_display();}
#endif
 if(pending_render_ack)assert(send_pending_render_ack());
}
static void reply(unsigned kind,unsigned epoch,unsigned inc,unsigned session,const char *mode,bool legacy){
 char context[260]="";
 if(!legacy)snprintf(context,sizeof context,"\"input_context\":{\"incarnation\":%u,\"session_epoch\":%u,\"display_epoch\":%u,\"mode\":\"%s\"},",inc,session,epoch,mode);
 if(kind==10)snprintf(json,sizeof json,"{\"type\":\"input_accepted\",\"data\":{%s\"display_epoch\":%u,\"selected\":\"weather\"}}",context,epoch);
 else snprintf(json,sizeof json,"{\"type\":\"plugin_position\",\"data\":{%s\"plugin\":\"weather\",\"dataset_id\":\"fixture\",\"target_date\":\"2026-10-14\",\"index\":0}}",context);
 process(kind,json);
}
#if !defined(PM_TEST_OLD_FIRMWARE) || !PM_TEST_OLD_FIRMWARE
static void native_chunk(unsigned id,unsigned epoch,unsigned session,unsigned offset,unsigned count){
 uint8_t packet[1+50+44+256];assert(count<=256);size_t k=0,n=strlen(PM_DEVICE_ID);
 packet[k++]=(uint8_t)n;memcpy(packet+k,PM_DEVICE_ID,n);k+=n;
 uint64_t values[]={1,session,epoch,42,id};
 for(unsigned i=0;i<5;i++)for(unsigned b=0;b<8;b++)packet[k++]=(uint8_t)(values[i]>>(56-8*b));
 for(unsigned b=0;b<4;b++)packet[k++]=(uint8_t)(offset>>(24-8*b));
 memset(packet+k,0,count);process_frame_bytes(packet,k+count);
}
static void native_reception(void){
 setup();unsigned id=++serial;start(id,1,1,"normal");chunk(id,1,1,0,256);
 native_chunk(id,1,1,256,256);assert(assembly.used==512);
 reply(10,1,1,1,"normal",false);assert(assembly.used==512);
 native_chunk(id,0,1,512,256);native_chunk(id,1,2,512,256);assert(assembly.used==512);
 for(unsigned at=512;at<PM_IMAGE_BYTES;at+=256){unsigned n=PM_IMAGE_BYTES-at;if(n>256)n=256;native_chunk(id,1,1,at,n);}
#if PM_DISPLAY_TYPE == PM_DISPLAY_EINK
 if(drawing){panel_result=true;poll_display();}
#endif
 assert(send_pending_render_ack());assert(frame_receipts==1&&frame_renders==1);
 setup();id=++serial;start(id,1,1,"normal");
 uint8_t invalid[100]={255};process_frame_bytes(invalid,sizeof invalid);assert(assembly.active&&assembly.used==0);
 puts("native chunks: binary NUL/pixel bytes, mixed legacy, stale tokens, control interleave, invalid header PASS");
}
#endif
#if PM_DISPLAY_TYPE == PM_DISPLAY_LCD && (!defined(PM_TEST_OLD_FIRMWARE) || !PM_TEST_OLD_FIRMWARE)
static void stream_start(unsigned id,unsigned sequence){
 uint8_t zeros[PM_IMAGE_BYTES]={0};char token[400];token_json(token,sizeof token,id,1,1);
 snprintf(json,sizeof json,"{\"type\":\"frame_start\",\"data\":{%s,\"mode\":\"normal\",\"width\":%u,\"height\":%u,\"format\":\"%s\",\"byte_len\":%u,\"crc32\":%lu,\"stream\":{\"fps\":6,\"sequence\":%u,\"generation\":1}}}",token,PM_W,PM_H,PM_PIXEL_FORMAT_NAME,PM_IMAGE_BYTES,(unsigned long)pm_crc32(zeros,sizeof zeros),sequence);
 process(12,json);
}
static void stream_finish(unsigned id){
 for(unsigned at=0;at<PM_IMAGE_BYTES;at+=256){unsigned n=PM_IMAGE_BYTES-at;if(n>256)n=256;native_chunk(id,1,1,at,n);}
}
static void video_reception(void){
 setup();checking_frame=false;clock_ms=5000000;stream_status_count=0;last_video_status_us=0;
 unsigned id=++serial;stream_start(id,1);assert(video.active&&receiving_stream);
 assert(video.first_us==(uint64_t)clock_ms*1000 && video.first_us>UINT32_MAX);
#if PM_TRACE_ENABLE
 assert(pm_trace_clock()==time_us_64());
#else
 assert(pm_trace_clock()==0); // Disabled diagnostics are not a functional clock.
#endif
 stream_finish(id);assert(video.ready&&!frame_receipts&&!frame_renders&&!pending_render_ack);
 poll_video();assert(video.rendered==1&&!video.ready&&!frame_receipts&&!frame_renders);
 assert(video.last_draw_us==time_us_64()&&stream_status_count==1);
 uint64_t first=video.last_draw_us,deadline=video.next_us;
 id=++serial;stream_start(id,2);stream_finish(id);poll_video();
 assert(video.ready&&video.rendered==1&&video.next_us==deadline);
 clock_ms=(uint32_t)((deadline-1)/1000);poll_video();assert(video.ready&&video.rendered==1);
 clock_ms=(uint32_t)((deadline+999)/1000);poll_video();
 assert(!video.ready&&video.rendered==2&&video.last_draw_us>=deadline);
 assert(video.max_gap_us==video.last_draw_us-first&&!frame_receipts&&!frame_renders);
 uint64_t elapsed;clock_ms+=1100;poll_video();
 assert(video.rendered==2&&stream_status_count==2);
 assert(number(stream_status_last,"elapsed_ms",&elapsed)&&elapsed==(time_us_64()-video.first_us)/1000&&elapsed>=1000);
 // A completed but undisplayed image cannot win against a local selection.
 video.ready=true;button(3,false);assert(await_selection);poll_video();assert(!video.ready&&video.rendered==2);
 reply(10,2,1,1,"normal",false);poll_video();assert(!video.active);
 checking_frame=true;id=++serial;start(id,2,1,"normal");finish_image(id,2,1,0);
 assert(frame_receipts==1&&frame_renders==1&&!video.active);
 printf("production video clock trace=%u: >32-bit clock, second-frame deadline, elapsed/max-gap stats PASS\n",PM_TRACE_ENABLE);
 puts("production video parser: no per-frame ACK, local input suppression, static ACK restoration PASS");
}
#if PM_TRACE_STATS
static void recorded_frame_paths(void){
 setup();clock_ms+=1;unsigned id=100001;start(id,1,1,"normal");pm_rx_capture(time_us_64());
 uint8_t wrong=1;receive_chunk(assembly.token,0,&wrong,1);
 finish_image(id,1,1,1);assert(!assembly.active && !frame_receipts && !frame_renders);
 pm_rx_capture(time_us_64());pm_rx_record r;
 assert(pm_rx_snapshot_record(1,&r) && r.frame==id && r.reason==PM_RX_INVALID);
 setup();clock_ms++;start(++id,1,1,"normal");pm_rx_capture(time_us_64());
 reset_assembly();pm_rx_capture(time_us_64());
 assert(pm_rx_snapshot_record(1,&r) && r.frame==id && r.reason==PM_RX_RESET);
 setup();clock_ms++;start(++id,1,1,"normal");pm_rx_capture(time_us_64());clock_ms++;
 start(id+1,1,1,"normal");pm_rx_capture(time_us_64());
 assert(pm_rx_snapshot_record(1,&r) && r.frame==id && r.reason==PM_RX_REPLACED);
 assert(pm_rx_snapshot_record(3,&r) && r.frame==id+1 && r.reason==PM_RX_ACTIVE);
 puts("production frame RX records: valid CRC completion, CRC rejection, reset and replacement IDs PASS");
}
#endif
#endif
#if defined(PM_TEST_OLD_FIRMWARE) && PM_TEST_OLD_FIRMWARE
int PM_CONTROL_TEST_ENTRY(void){
 for(unsigned kind=10;kind<=17;kind+=7){
  setup();unsigned id=++serial;reply(kind,1,1,1,"sub",false);
  start(id,1,1,"sub");chunk(id,1,1,0,256);finish_image(id,1,1,256);
  assert(frame_receipts==1&&frame_renders==1);
 }
 puts("old deployed firmware accepts new context fields and frame/ACK contract PASS");return 0;
}
#else
int PM_CONTROL_TEST_ENTRY(void){
 native_reception();
#if PM_DISPLAY_TYPE == PM_DISPLAY_LCD
 video_reception();
#if PM_TRACE_STATS
 recorded_frame_paths();
#endif
#endif
 // Both response types, legacy/current contexts, before/after start and mid-image.
 for(unsigned kind=10;kind<=17;kind+=7)for(unsigned legacy=0;legacy<2;legacy++)for(unsigned order=0;order<3;order++){
  setup();unsigned id=++serial;
  if(order==0)reply(kind,1,1,1,"sub",legacy);
  start(id,1,1,"sub");assert(assembly.active);
  if(order==1)reply(kind,1,1,1,"sub",legacy);
  chunk(id,1,1,0,256);assert(assembly.active&&assembly.used==256);
  if(order==2)reply(kind,1,1,1,"sub",legacy);
  assert(assembly.active&&assembly.used==256);
  finish_image(id,1,1,256);assert(frame_receipts==1&&frame_renders==1);
 }
 // Delayed old generations/sessions/incarnations never change mode or assembly.
 setup();unsigned id=++serial;start(id,1,1,"sub");chunk(id,1,1,0,256);
 reply(17,0,1,1,"normal",false);reply(10,1,2,1,"normal",false);reply(17,1,1,2,"normal",false);
 assert(sub_mode&&assembly.active&&assembly.used==256);
 chunk(id-1,1,1,256,256);assert(assembly.active&&assembly.used==256);
 finish_image(id,1,1,256);assert(frame_renders==1);
 // New Web/selection generation invalidates the old partial image, keeps the new.
 setup();id=++serial;start(id,1,1,"sub");chunk(id,1,1,0,256);
 reply(10,2,1,1,"normal",false);assert(!assembly.active&&!sub_mode);
 finish_image(id,1,1,256);assert(!frame_receipts);
 start(++serial,2,1,"normal");id=serial;chunk(id,2,1,0,256);
 reply(17,1,1,1,"sub",false);assert(assembly.active&&!sub_mode);
 finish_image(id,2,1,256);assert(frame_receipts==1&&frame_renders==1);
 // A newer valid start may arrive before the physical button reply.
 setup();button(3,false);assert(await_selection);start(++serial,1,1,"normal");assert(!assembly.active);
 id=++serial;start(id,2,1,"normal");assert(assembly.active&&!await_selection);
 chunk(id,2,1,0,256);reply(10,2,1,1,"normal",false);assert(assembly.used==256);
 // Repeated switching: a later generation wins; delayed reply/old bytes do not.
 button(3,false);start(++serial,3,1,"normal");id=serial;chunk(id,3,1,0,256);
 reply(10,2,1,1,"normal",false);chunk(id-1,2,1,256,256);assert(assembly.active&&assembly.used==256);
 finish_image(id,3,1,256);assert(frame_receipts==1&&frame_renders==1);
 // SUB cancel through POWER; normal frame before the position reply survives.
 setup();sub_mode=true;button(2,false);assert(pending_sub_cancel&&await_selection);
 id=++serial;start(id,2,1,"normal");chunk(id,2,1,0,256);
 reply(17,2,1,1,"normal",false);assert(!sub_mode&&!pending_sub_cancel&&assembly.active);
 finish_image(id,2,1,256);assert(frame_receipts==1&&frame_renders==1);
 // Standby rejects even a newer frame; reconnection rejects old-session control.
 setup();button(2,false);start(++serial,2,1,"normal");assert(!assembly.active);
 reply(10,2,1,1,"standby",false);assert(state==STANDBY&&!assembly.active);
 setup();session_epoch=2;id=++serial;start(id,1,2,"sub");chunk(id,1,2,0,256);
 reply(17,2,1,1,"normal",false);assert(sub_mode&&assembly.used==256);
 finish_image(id,1,2,256);assert(frame_receipts==1&&frame_renders==1);
 // Settings sync is independent of the active image and its token.
 setup();id=++serial;start(id,1,1,"normal");chunk(id,1,1,0,256);
 process(3,"{\"type\":\"settings\",\"data\":{\"incarnation\":1,\"revision\":2,\"wifi_timeout_seconds\":15,\"app_timeout_seconds\":10,\"retry_initial_seconds\":5,\"retry_max_seconds\":30,\"heartbeat_seconds\":15,\"offline_seconds\":45,\"two_button_long_press_millis\":1800}}");
 assert(settings.revision==2&&assembly.active&&assembly.used==256);
 finish_image(id,1,1,256);assert(frame_receipts==1&&frame_renders==1);
 // Bad matching offsets still invalidate; protection must not bless corruption.
 setup();id=++serial;start(id,1,1,"normal");chunk(id,1,1,256,256);assert(!assembly.active);
 assert(!frame_receipts&&!frame_renders);
 puts("control replies: order/legacy/stale/rapid switch/Web/cancel/standby/settings/reconnect/invalid offset PASS");
 return 0;
}

#endif
