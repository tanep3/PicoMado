/* Real wire bridge with bounded fake runtime; no device or socket access. */
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "bluetooth_runtime.h"
#include "device_api.h"
#define PM_BLUETOOTH_AUDIO 1
#define ACTIVE 1
static int state=ACTIVE;
static uint64_t incarnation=1,session_epoch=2,display_epoch=3;
static bool bt_initialized=true,begin_ok=true,send_ok=true;
static uint32_t now_ms;
static unsigned locks,reports,plays,ends;
static uint64_t consumed;
static pm_bluetooth_audio ring;
static char outbound_json[768],last_report[768];
static uint32_t get_absolute_time(void){return now_ms;}
static uint32_t to_ms_since_boot(uint32_t n){return n;}
static void cyw43_arch_lwip_begin(void){assert(!locks);++locks;}
static void cyw43_arch_lwip_end(void){assert(locks==1);--locks;}
uint32_t pm_bt_runtime_rate(void){assert(locks);return 48000;}
bool pm_bt_runtime_audio_begin(uint64_t gen,uint64_t first){assert(locks);if(!begin_ok)return false;assert(pm_bluetooth_audio_start(&ring,gen,48000));ring.next_frame=first;consumed=first;return true;}
void pm_bt_runtime_audio_end(void){assert(locks);++ends;pm_bluetooth_audio_stop(&ring);}
pm_bluetooth_audio_result pm_bt_runtime_audio_push(uint64_t gen,uint64_t first,const int16_t *pcm,size_t frames){assert(locks);return pm_bluetooth_audio_push(&ring,gen,first,pcm,frames);}
uint64_t pm_bt_runtime_audio_next_frame(void){assert(locks);return ring.next_frame;}
uint64_t pm_bt_runtime_audio_consumed_frames(void){assert(locks);return consumed;}
bool pm_bt_runtime_audio_active(void){assert(locks);return ring.active;}
bool pm_bt_runtime_audio_finish(uint64_t gen,uint64_t end){assert(locks);return ring.active&&gen==ring.generation&&end>=ring.next_frame;}
uint8_t pm_bt_runtime_play(void){assert(locks);++plays;return 0;}
static bool pm_net_send(unsigned kind,const char *j){assert(!locks&&kind==25);++reports;snprintf(last_report,sizeof last_report,"%s",j);return send_ok;}
static bool number(const char *j,const char *key,uint64_t *v){char needle[80];snprintf(needle,sizeof needle,"\"%s\":",key);const char *p=strstr(j,needle);if(!p)return false;char *end;*v=strtoull(p+strlen(needle),&end,10);return end!=p+strlen(needle);}
void pm_bt_runtime_link_snapshot(pm_bt_link_snapshot *out){assert(locks);memset(out,0,sizeof *out);}
void pm_bt_runtime_audio_diagnostics(pm_bt_audio_diagnostics *out){assert(locks);memset(out,0,sizeof *out);}
static void pm_logf(const char *fmt,...){(void)fmt;assert(!locks);}
#include "../src/bluetooth_media.inc"
static void reset(void){memset(&bt_media,0,sizeof bt_media);memset(&ring,0,sizeof ring);state=ACTIVE;incarnation=1;session_epoch=2;display_epoch=3;bt_initialized=begin_ok=send_ok=true;now_ms=locks=reports=plays=ends=0;consumed=0;last_report[0]=0;}
static pm_api_result command(unsigned kind,uint64_t stream,uint64_t value){char j[256];snprintf(j,sizeof j,"{\"incarnation\":1,\"session_epoch\":2,\"display_epoch\":3,\"stream_id\":%llu,\"first_frame\":%llu,\"final_frame\":%llu,\"rate\":48000}",(unsigned long long)stream,(unsigned long long)value,(unsigned long long)value);return api_audio(kind,j);}
static void be64(uint8_t *p,uint64_t v){for(unsigned i=0;i<8;i++){p[7-i]=(uint8_t)v;v>>=8;}}
static void chunk(uint64_t stream,uint64_t first,size_t frames){assert(frames<=256);uint8_t p[1064];be64(p,1);be64(p+8,2);be64(p+16,3);be64(p+24,stream);be64(p+32,first);for(size_t i=0;i<frames;i++){p[40+i*4]=0x34;p[41+i*4]=0x12;p[42+i*4]=0xfe;p[43+i*4]=0xff;}process_audio_bytes(p,40+4*frames);}
static void consume(unsigned frames){int16_t pcm[512];assert(frames<=256&&pm_bluetooth_audio_pop(&ring,pcm,frames));consumed+=frames;}
static void test_identity_endian_credit(void){
 reset();assert(command(22,1,100)==PM_API_PENDING);assert(command(22,1,100)==PM_API_IGNORED);
 chunk(2,100,128);assert(!ring.used);chunk(1,100,128);assert(ring.used==128&&ring.samples[0]==0x1234&&ring.samples[1]==-2);
 chunk(1,100,128);assert(ring.used==128);now_ms=10;poll_audio();assert(!plays&&strstr(last_report,"\"accepted\":228")&&strstr(last_report,"\"consumed\":100"));
 now_ms=30;poll_audio();assert(!plays);assert(command(26,1,228)==PM_API_PENDING);poll_audio();assert(plays==1);consume(128);now_ms=40;poll_audio();assert(strstr(last_report,"\"consumed\":228"));
 session_epoch=4;assert(command(22,2,0)==PM_API_IGNORED);poll_audio();assert(!bt_media.active&&ends==1);
}
static void test_finish(void){
 reset();command(22,1,10);assert(command(26,1,137)==PM_API_REJECTED);assert(command(26,2,138)==PM_API_IGNORED);
 assert(command(26,1,138)==PM_API_PENDING);assert(command(26,1,266)==PM_API_REJECTED);
 chunk(1,10,128);now_ms=30;poll_audio();assert(plays==1);consume(128);ring.active=false;now_ms=40;poll_audio();
 assert(!bt_media.active&&!bt_media.failed&&strstr(last_report,"\"failed\":false")&&strstr(last_report,"\"consumed\":138"));
 reset();command(22,1,0);command(26,1,128);chunk(1,0,256);assert(bt_media.failed&&!bt_media.active);
 reset();command(22,1,0);command(26,1,256);chunk(1,0,128);consume(128);ring.active=false;now_ms=40;poll_audio();assert(bt_media.failed);
}
static void test_overflow_gap_start_cancel(void){
 reset();command(22,1,0);for(unsigned i=0;i<PM_BT_PCM_FRAMES/256;i++)chunk(1,i*256,256);assert(ring.used==PM_BT_PCM_FRAMES);chunk(1,PM_BT_PCM_FRAMES,1);assert(bt_media.failed&&!bt_media.active);
 reset();command(22,1,UINT64_MAX-1);chunk(1,UINT64_MAX-1,2);assert(bt_media.failed);
 reset();command(22,1,0);chunk(1,1,128);assert(bt_media.failed);
 reset();begin_ok=false;assert(command(22,1,0)==PM_API_REJECTED);now_ms=10;poll_audio();assert(strstr(last_report,"\"failed\":true"));
 reset();command(22,1,0);display_epoch=4;poll_audio();assert(!bt_media.active&&ends==1&&!reports);chunk(1,0,128);assert(!ring.used);
 reset();command(22,1,0);assert(command(23,2,0)==PM_API_COMPLETED&&bt_media.active);command(23,1,0);assert(!bt_media.active&&!bt_media.failed);
}
static void test_report_retry(void){reset();command(22,1,0);send_ok=false;now_ms=10;poll_audio();assert(bt_media.dirty);send_ok=true;now_ms=20;poll_audio();assert(!bt_media.dirty&&reports==2);}
static void test_full_prefill(void){
 reset();command(22,1,0);
 for(unsigned i=0;i<PM_BT_PCM_FRAMES/256-1;i++){chunk(1,i*256,256);now_ms+=10;poll_audio();assert(!plays);}
 chunk(1,PM_BT_PCM_FRAMES-256,256);poll_audio();assert(plays==1&&ring.used==PM_BT_PCM_FRAMES);
}
int main(void){test_full_prefill();test_identity_endian_credit();test_finish();test_overflow_gap_start_cancel();test_report_retry();assert(!locks);puts("Bluetooth media: identity, LE PCM, credit, EOF, overflow, start failure, cancellation PASS");}
