#define PM_APP_TEST_ENTRY unused_app_fixture
#include "app_timing_test.c"
void pm_display_poll(void){}
bool pm_display_busy(void){return false;}
bool pm_display_take_result(bool *ok){(void)ok;return false;}
bool pm_display_clock_frame(uint8_t *p,size_t n,unsigned m,uint64_t v,bool r){(void)m;(void)v;(void)r;return pm_display_frame(p,n);}
bool pm_display_video_frame(uint8_t *p,size_t n,unsigned m,uint64_t v,bool r){(void)m;(void)v;(void)r;return pm_display_frame(p,n);}
#if PM_HOME_STATIC_ARP_DIAGNOSTIC
static pm_profile_id seen_profile;
static const pm_profile *seen_connection;
void pm_net_begin_home_static_arp(const pm_profile *p,pm_profile_id id){
    seen_profile=id;seen_connection=p;server_begins++;network_stage=PM_NET_TCP;
}
#endif
int main(void){
    for(unsigned i=0;i<PM_PROFILE_COUNT;i++){
        #if !PM_HOME_STATIC_ARP_DIAGNOSTIC
        if(i==PM_PROFILE_AWAY)continue; // The baseline fixture models AWAY through EEX1 separately.
#endif
        active_profile=(uint8_t)i;unsigned old=server_begins;
        start_server_now(&profiles[i]);assert(server_begins==old+1);
#if PM_HOME_STATIC_ARP_DIAGNOSTIC
        assert(seen_profile==(pm_profile_id)i && seen_connection==&profiles[i]);
#endif
    }
    puts("PASS main dispatch: explicit HOME/HOME2/AWAY profile, no profile persistence changes");
}
