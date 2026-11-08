#define PM_APP_TEST_ENTRY unused_base_app_test
#include "app_timing_test.c"
static bool panel_busy,apply_failure,cleanup_failure,cache_program_failure;
static unsigned pin_calls,without_hint_calls,failed_calls,cache_programs,cache_erases;
static uint8_t cache_media[4096];
#if PM_BLUETOOTH_AUDIO
static bool runtime_ready=true,runtime_audio;
bool pm_bt_runtime_ready(void){assert(lwip_depth);return runtime_ready;}
bool pm_bt_runtime_audio_active(void){assert(lwip_depth);return runtime_audio;}
void pm_bt_runtime_audio_diagnostics(pm_bt_audio_diagnostics *d){(void)d;assert(!"audio not exercised");}
void pm_bt_runtime_link_snapshot(pm_bt_link_snapshot *d){(void)d;assert(!"audio not exercised");}
void pm_bt_runtime_audio_end(void){assert(!"audio not exercised");}
bool pm_bt_runtime_audio_finish(uint64_t g,uint64_t f){(void)g;(void)f;assert(!"audio not exercised");return false;}
uint32_t pm_bt_runtime_rate(void){assert(!"audio not exercised");return 0;}
bool pm_bt_runtime_audio_sbc_begin(uint64_t g,uint64_t f,pm_bt_sbc_config *c,uint64_t *e){(void)g;(void)f;(void)c;(void)e;assert(!"audio not exercised");return false;}
bool pm_bt_runtime_audio_begin(uint64_t g,uint64_t f){(void)g;(void)f;assert(!"audio not exercised");return false;}

#endif
void pm_display_poll(void){}
bool pm_display_busy(void){return panel_busy;}
bool pm_display_take_result(bool *ok){(void)ok;return false;}
bool pm_display_clock_frame(uint8_t *p,size_t n,unsigned m,uint64_t v,bool r){(void)m;(void)v;(void)r;return pm_display_frame(p,n);}
bool pm_display_video_frame(uint8_t *p,size_t n,unsigned m,uint64_t v,bool r){(void)m;(void)v;(void)r;return pm_display_frame(p,n);}
void pm_net_begin_profile(const pm_profile *p,pm_profile_id id){assert(p==&profiles[id]);server_begins++;}
void pm_net_server_link_query(pm_server_link_scope *q){assert(state==SESSION);*q=(pm_server_link_scope){.connection=42};}
bool pm_net_server_link_apply(const pm_server_link_scope *q,const pm_server_link_hint *h,uint64_t inc,uint64_t session){
 assert(state==SESSION&&q->connection==42&&h->server_ipv4==0xc0a80078&&inc==3&&session==100);pin_calls++;
 assert(!cache_programs);return !apply_failure;
}
bool pm_net_server_link_without_hint(void){assert(state==SESSION);without_hint_calls++;return !cleanup_failure;}
void pm_net_server_link_connection_failed(void){failed_calls++;}
bool pm_net_server_link_remembered(pm_server_link_cache_record *r){
 return pm_server_link_cache_get((pm_profile_id)active_profile,(const uint8_t *)profiles[active_profile].ssid,strlen(profiles[active_profile].ssid),0xc0a80078,r);
}
static pm_server_link_cache_result read_cache_io(void *x,uint32_t at,uint8_t *out,size_t n){(void)x;assert(at+n<=sizeof cache_media);memcpy(out,cache_media+at,n);return PM_SERVER_LINK_CACHE_OK;}
static pm_server_link_cache_result erase_cache_io(void *x){(void)x;assert(!lwip_depth&&!panel_busy&&state!=SESSION);cache_erases++;memset(cache_media,255,sizeof cache_media);return PM_SERVER_LINK_CACHE_OK;}
static pm_server_link_cache_result program_cache_io(void *x,uint32_t at,const uint8_t *page){
 (void)x;assert(!lwip_depth&&!panel_busy&&state!=SESSION&&at+256<=sizeof cache_media);cache_programs++;
 if(cache_program_failure)return PM_SERVER_LINK_CACHE_SAFETY_FAILED;
 for(unsigned i=0;i<256;i++){assert((cache_media[at+i]&page[i])==page[i]);cache_media[at+i]&=page[i];}return PM_SERVER_LINK_CACHE_OK;
}
const pm_server_link_cache_io *pm_server_link_cache_flash_io(void){static const pm_server_link_cache_io io={.read=read_cache_io,.erase=erase_cache_io,.program=program_cache_io};return &io;}
static void session(void){defaults();settings.incarnation=3;state=SESSION;incarnation=session_epoch=0;active_profile=PM_PROFILE_HOME;network_stage=PM_NET_READY;}
static void remember_mac(uint8_t last){pm_server_link_hint h={.server_ipv4=0xc0a80078,.prefix_length=24,.mac={2,0,0,0,0,last}};cache_remember(&h);}
int main(void){
 memset(cache_media,255,sizeof cache_media);memset(test_flash_sector,0x5a,sizeof test_flash_sector);
 assert(pm_server_link_cache_load(pm_server_link_cache_flash_io())==PM_SERVER_LINK_CACHE_EMPTY);
 const char *old="{\"type\":\"welcome\",\"data\":{\"incarnation\":3,\"session_epoch\":100}}";
 const char *valid="{\"type\":\"welcome\",\"data\":{\"incarnation\":3,\"session_epoch\":100,\"server_link\":{\"version\":1,\"server_ipv4\":\"192.168.0.120\",\"client_ipv4\":\"192.168.0.44\",\"prefix_length\":24,\"mac\":\"2c:cf:67:11:72:f3\"}}}";
 const char *invalid="{\"type\":\"welcome\",\"data\":{\"incarnation\":3,\"session_epoch\":100,\"server_link\":{}}}";
 session();assert(api_welcome(2,old)==PM_API_COMPLETED&&without_hint_calls==1&&!pm_server_link_cache_dirty());
 session();assert(api_welcome(2,invalid)==PM_API_COMPLETED&&without_hint_calls==2&&!pm_server_link_cache_dirty());
 session();apply_failure=true;assert(api_welcome(2,valid)==PM_API_COMPLETED&&!pm_server_link_cache_dirty());apply_failure=false;
 session();clock_ms=1000;assert(api_welcome(2,valid)==PM_API_COMPLETED&&state==ACTIVE&&pm_server_link_cache_dirty());
 assert(!cache_programs&&!cache_erases);assert(api_welcome(2,valid)==PM_API_IGNORED);
 for(unsigned p=0;p<PM_PROFILE_COUNT;p++){
  active_profile=p;assert(send_hello());
  assert((strstr(outbound_json,PM_SERVER_LINK_CACHE_CAPABILITY)!=NULL)==(p!=PM_PROFILE_AWAY));
  assert((strstr(outbound_json,"\"remembered_server_link\"")!=NULL)==(p==PM_PROFILE_HOME));
  assert(!strstr(outbound_json,",\\\""));if(p==PM_PROFILE_HOME)printf("HELLO %s\n",outbound_json);
 }
 active_profile=PM_PROFILE_HOME;clock_ms=5999;cache_save_when_idle();assert(!cache_programs);
 clock_ms=6000;panel_busy=true;cache_save_when_idle();assert(!cache_programs);panel_busy=false;
 state=SESSION;cache_save_when_idle();assert(!cache_programs);clock_ms=11000;state=ACTIVE;cache_save_when_idle();assert(!cache_programs);
#if PM_BLUETOOTH_AUDIO
 clock_ms=11000;bt_waiting=true;cache_save_when_idle();assert(!cache_programs);bt_waiting=false;
 bt_pending=true;cache_save_when_idle();assert(!cache_programs);bt_pending=false;
 bt_initialized=true;runtime_ready=false;cache_save_when_idle();assert(!cache_programs);runtime_ready=true;
 runtime_audio=true;cache_save_when_idle();assert(!cache_programs);runtime_audio=false;
 bt_media.active=true;cache_save_when_idle();assert(!cache_programs);bt_media.active=false;bt_initialized=false;
#endif
 clock_ms=11000;cache_save_when_idle();clock_ms=16000;cache_save_when_idle();assert(cache_programs==2&&!pm_server_link_cache_dirty());
 unsigned saved=cache_programs;clock_ms=20000;remember_mac(1);assert(pm_server_link_cache_dirty());cache_save_when_idle();
 clock_ms=75999;cache_save_when_idle();assert(cache_programs==saved);clock_ms=76000;cache_save_when_idle();assert(cache_programs==saved+2);
 saved=cache_programs;remember_mac(1);assert(!pm_server_link_cache_dirty());clock_ms=140000;cache_save_when_idle();assert(cache_programs==saved);
 remember_mac(2);cache_save_when_idle();clock_ms=145000;cache_program_failure=true;cache_save_when_idle();assert(pm_server_link_cache_dirty()&&cache_programs==saved+1);
 clock_ms=146000;cache_program_failure=false;cache_save_when_idle();assert(cache_programs==saved+1);
 clock_ms=205000;cache_save_when_idle();assert(!pm_server_link_cache_dirty());
 for(unsigned i=0;i<sizeof test_flash_sector;i++)assert(test_flash_sector[i]==0x5a);
 session();cleanup_failure=true;unsigned failed=failed_calls;assert(api_welcome(2,old)==PM_API_ERROR&&state==RETRY&&failed_calls==failed+1);
 cleanup_failure=false;session();retry_with_image(false);assert(failed_calls==failed+2&&state==RETRY);
 puts("PASS production cache app: Hello/old peer, RAM-before-save, duplicate/invalid rejection, idle guards/rate, failed write retry, old settings untouched");
}
