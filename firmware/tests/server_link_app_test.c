#define PM_APP_TEST_ENTRY unused_app_fixture
#include "app_timing_test.c"
void pm_display_poll(void){}
bool pm_display_busy(void){return false;}
bool pm_display_take_result(bool *ok){(void)ok;return false;}
bool pm_display_clock_frame(uint8_t *p,size_t n,unsigned m,uint64_t v,bool r){(void)m;(void)v;(void)r;return pm_display_frame(p,n);}
bool pm_display_video_frame(uint8_t *p,size_t n,unsigned m,uint64_t v,bool r){(void)m;(void)v;(void)r;return pm_display_frame(p,n);}
static unsigned applied,queried;
static bool fail_apply;
#if PM_SERVER_LINK_HINT
void pm_net_begin_profile(const pm_profile *p,pm_profile_id id){assert(p==&profiles[id]);server_begins++;}
void pm_net_server_link_query(pm_server_link_scope *q){assert(state==SESSION);queried++;*q=(pm_server_link_scope){.connection=42};}
bool pm_net_server_link_apply(const pm_server_link_scope *q,const pm_server_link_hint *h,uint64_t inc,uint64_t session){
    assert(state==SESSION&&q->connection==42&&h->server_ipv4==0xc0a80078&&inc==3&&session==100);
    assert(!frame_receipts&&!frame_renders);applied++;return !fail_apply;
}
#endif
static void fresh(void){defaults();settings.incarnation=3;state=SESSION;incarnation=session_epoch=0;active_profile=PM_PROFILE_HOME;applied=queried=0;}
int main(void){
    const char *old="{\"type\":\"welcome\",\"data\":{\"incarnation\":3,\"session_epoch\":100}}";
    const char *extra="{\"type\":\"welcome\",\"data\":{\"incarnation\":3,\"session_epoch\":100,\"server_link\":{\"version\":1,\"server_ipv4\":\"192.168.0.120\",\"client_ipv4\":\"192.168.0.44\",\"prefix_length\":24,\"mac\":\"2c:cf:67:11:72:f3\"}}}";
    const char *bad_hint="{\"type\":\"welcome\",\"data\":{\"incarnation\":3,\"session_epoch\":100,\"server_link\":{}}}";
    for(unsigned i=0;i<3;i++){
        fresh();assert(api_welcome(2,i==0?old:i==1?extra:bad_hint)==PM_API_COMPLETED);
        assert(state==ACTIVE&&incarnation==3&&session_epoch==100);
#if PM_SERVER_LINK_HINT
        assert(queried==1&&applied==(i==1));
#else
        assert(!queried&&!applied);
#endif
        assert(api_welcome(2,extra)==PM_API_IGNORED);
    }
    fresh();fail_apply=true;assert(api_welcome(2,extra)==PM_API_COMPLETED&&state==ACTIVE);fail_apply=false;
    for(unsigned i=0;i<PM_PROFILE_COUNT;i++){
        active_profile=(uint8_t)i;network_stage=PM_NET_READY;assert(send_hello());
        bool capability=strstr(outbound_json,PM_SERVER_LINK_CAPABILITY)!=NULL;
        assert(capability==(PM_SERVER_LINK_HINT&&(i==PM_PROFILE_HOME||i==PM_PROFILE_HOME2)));
    }
    puts("PASS production app: profile capability, old/new/invalid hint fallback, apply before ACTIVE, duplicate ignored, BT-off main integration");
}
