#if defined(PM_CACHE_APP_DEADLINE)
/* Execute the production main loop and exact shared SERVER/SESSION deadline.
 * This mode mocks the network boundary; owner mode below exercises that API. */
#define gpio_init watched_gpio_init
#define gpio_set_dir watched_gpio_set_dir
#define gpio_pull_up watched_gpio_pull_up
#define sleep_ms watched_sleep
#define pm_net_close watched_close
#define pm_net_take watched_take
#define main pm_firmware_entry
#ifndef PM_CACHE_APP_MAIN_SOURCE
#define PM_CACHE_APP_MAIN_SOURCE "../src/main.c"
#endif
#include PM_CACHE_APP_MAIN_SOURCE
#undef main
#undef gpio_init
#undef gpio_set_dir
#undef gpio_pull_up
#undef sleep_ms
#undef pm_net_close
#undef pm_net_take
#define PM_APP_MAIN_SOURCE "../src/server_link_cache.h"
#define PM_APP_TEST_ENTRY baseline_app_main
#include "app_timing_test.c"
/* The shared baseline targets LCD GPIO12..15; this mode validates the actual
 * selected E-ink button pins instead of relaxing the baseline adapter. */
void watched_gpio_init(uint pin){assert(pin==PM_BUTTON_UP_PIN||pin==PM_BUTTON_DOWN_PIN);}
void watched_gpio_set_dir(uint pin,int direction){watched_gpio_init(pin);assert(direction==GPIO_IN);}
void watched_gpio_pull_up(uint pin){watched_gpio_init(pin);}
static unsigned deadline_phase,deadline_kind,deadline_seconds;
static unsigned failure_calls,close_after_failure,order,failure_order,close_order;
static uint32_t original_started;
static bool waiting_attempt;
void pm_display_poll(void){}
bool pm_display_busy(void){return false;}
bool pm_display_take_result(bool *ok){(void)ok;return false;}
bool pm_display_clock_frame(uint8_t *p,size_t n,unsigned m,uint64_t v,bool r){(void)m;(void)v;(void)r;return pm_display_frame(p,n);}
bool pm_display_video_frame(uint8_t *p,size_t n,unsigned m,uint64_t v,bool r){(void)m;(void)v;(void)r;return pm_display_frame(p,n);}
void pm_net_begin_profile(const pm_profile *p,pm_profile_id id){
    assert(p==&profiles[id]&&id==PM_PROFILE_HOME);pm_net_begin(p);waiting_attempt=true;
}
void pm_net_server_link_query(pm_server_link_scope *q){(void)q;assert(!"No Welcome is delivered in a deadline test");}
bool pm_net_server_link_apply(const pm_server_link_scope *q,const pm_server_link_hint *h,uint64_t i,uint64_t s){
    (void)q;(void)h;(void)i;(void)s;assert(!"No Welcome is delivered");return false;
}
bool pm_net_server_link_without_hint(void){assert(!"No Welcome is delivered");return true;}
bool pm_net_server_link_remembered(pm_server_link_cache_record *r){(void)r;return false;}
void pm_net_server_link_connection_failed(void){
    assert(waiting_attempt&&network_stage!=PM_NET_DOWN);
    assert(state==(deadline_kind==2?SESSION:SERVER));
    assert(started_ms==original_started&&clock_ms-started_ms==deadline_seconds*1000u+1);
    failure_calls++;failure_order=++order;
}
void watched_close(void){
    if(waiting_attempt){assert(failure_calls==1&&failure_order&&!close_after_failure);close_order=++order;close_after_failure++;}
    pm_net_close();
}
bool watched_take(uint8_t *k,char *j,size_t n){(void)k;(void)j;(void)n;return false;}
static pm_server_link_cache_result empty_read(void *c,uint32_t off,uint8_t *p,size_t n){
    (void)c;assert(off+n<=4096);memset(p,255,n);return PM_SERVER_LINK_CACHE_OK;
}
static pm_server_link_cache_result no_erase(void *c){(void)c;assert(!"No cache write during handshake");return PM_SERVER_LINK_CACHE_ERASE_FAILED;}
static pm_server_link_cache_result no_program(void *c,uint32_t off,const uint8_t *p){(void)c;(void)off;(void)p;assert(!"No cache write during handshake");return PM_SERVER_LINK_CACHE_PROGRAM_FAILED;}
const pm_server_link_cache_io *pm_server_link_cache_flash_io(void){
    static const pm_server_link_cache_io io={NULL,empty_read,no_erase,no_program};return &io;
}
void watched_sleep(unsigned ms){
    assert(ms==5&&++loops<20);
    if(deadline_phase==0){
        assert(state==WIFI&&joins==1);settings.app_s=deadline_seconds;ip_link=wifi_link=CYW43_LINK_UP;
    }else if(deadline_phase==1){
        assert(state==SERVER&&network_stage==PM_NET_TCP&&server_begins==1);
        original_started=started_ms;
        clock_ms=started_ms+settings.app_s*1000u-(deadline_kind==2?1u:0u);
        if(deadline_kind==1)network_stage=PM_NET_WS;
        if(deadline_kind==2)server_ready=true;
    }else if(deadline_phase==2){
        assert(!failure_calls&&started_ms==original_started);
        assert(state==(deadline_kind==2?SESSION:SERVER));
        assert(hello_count==(deadline_kind==2?1u:0u));clock_ms++;
    }else if(deadline_phase==3&&deadline_kind==2){
        assert(state==SESSION&&started_ms==original_started&&!failure_calls);clock_ms++;
    }else {
        assert(state==RETRY&&network_stage==PM_NET_DOWN&&sta_on&&wifi_enabled&&joins==1);
        assert(failure_calls==1&&close_after_failure==1&&failure_order<close_order);
        longjmp(finished,1);
    }
    deadline_phase++;
}
int main(int argc,char **argv){
    assert(argc==3);assert(sscanf(argv[1],"%u",&deadline_kind)==1&&deadline_kind<=2);
    assert(sscanf(argv[2],"%u",&deadline_seconds)==1);
    assert(deadline_seconds==3||deadline_seconds==10||deadline_seconds==120);
    memset(test_flash_sector,255,sizeof test_flash_sector);
    if(!setjmp(finished))pm_firmware_entry();
    assert(failure_calls==1&&close_after_failure==1&&!lwip_depth&&!erases);
    printf("PASS production app deadline: phase=%u app_s=%u nonextending, boundary+1 -> failure notification -> close\n",deadline_kind,deadline_seconds);
    return 0;
}
#else
/* BT-AUDIO-20261107: actual net owner + cache aggregate; host IO only.
 * Strict adapters observe product calls, never repair/assert-away owner state. */
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include "lwip/etharp.h"
static struct tcp_pcb *watched_new(int type);
static err_t watched_connect(struct tcp_pcb *,const ip_addr_t *,uint16_t,tcp_connected_fn);
static int watched_find(struct netif *,const ip4_addr_t *,struct eth_addr **,const ip4_addr_t **);
static uint64_t watched_clock(void);
#define tcp_new_ip_type watched_new
#define tcp_connect watched_connect
#define etharp_find_addr watched_find
#define time_us_64 watched_clock
#ifndef PM_CACHE_OWNER_NET_SOURCE
#define PM_CACHE_OWNER_NET_SOURCE "../src/net.c"
#endif
#include PM_CACHE_OWNER_NET_SOURCE
#undef tcp_new_ip_type
#undef tcp_connect
#undef etharp_find_addr
#undef time_us_64
/* Reuse the original deterministic TCP SDK adapters without including net twice. */
#define PM_NET_SOURCE "../src/server_link_cache.h"
#define main baseline_net_main
#include "net_nagle_test.c"
#undef main

static struct netif route,other_route;
static unsigned adds,removes,reads,programs,erases;
static unsigned event_tick,add_tick,readback_tick,alloc_tick,connect_tick;
static bool bad_readback,expect_seed;
static err_t add_result,remove_result;
static uint64_t now_us=1000000;
static uint8_t disk[PM_SERVER_LINK_CACHE_SECTOR_SIZE];
static const pm_profile direct={"test","test","192.168.0.120",false,NULL,NULL,NULL,0};
static const pm_profile socks={"test","test","192.168.0.120",true,"198.51.100.2","test","test",1080};
static const pm_server_link_hint original={.server_ipv4=0xc0a80078,.client_ipv4=0xc0a8002c,.prefix_length=24,.mac={2,3,4,5,6,7}};
static uint64_t watched_clock(void){return now_us;}
static struct tcp_pcb *watched_new(int type){
    alloc_tick=++event_tick;
    if(expect_seed){assert(server_link.owned&&server_link.provisional);assert(add_tick&&readback_tick>add_tick&&alloc_tick>readback_tick);}
    return tcp_new_ip_type(type);
}
static err_t watched_connect(struct tcp_pcb *p,const ip_addr_t *ip,uint16_t port,tcp_connected_fn cb){
    connect_tick=++event_tick;
    if(expect_seed)assert(add_tick<readback_tick&&readback_tick<alloc_tick&&alloc_tick<connect_tick);
    /* Real lwIP populates endpoints during tcp_connect, before any callback. */
    p->local_ip=route.ip;p->remote_ip=*ip;
    return tcp_connect(p,ip,port,cb);
}
static int watched_find(struct netif *n,const ip4_addr_t *ip,struct eth_addr **mac,const ip4_addr_t **resolved){
    assert(lock_depth==1);readback_tick=++event_tick;reads++;
    return etharp_find_addr(n,ip,mac,resolved);
}
err_t etharp_add_static_entry(const ip4_addr_t *ip,struct eth_addr *mac){
    assert(lock_depth==1);adds++;add_tick=++event_tick;
    if(!pcb){assert(!alloc_tick&&!connect_tick);assert(ip->addr==server_addr.addr);}
    else {assert(stage==PM_NET_READY);assert(ip->addr==pcb->remote_ip.addr);}
    if(add_result)return add_result;
    fake_arp_found=true;fake_arp_mac=*mac;
    if(bad_readback)fake_arp_mac.addr[5]^=1;
    return ERR_OK;
}
err_t etharp_remove_static_entry(const ip4_addr_t *ip){
    assert(lock_depth==1&&ip->addr==server_link.address.addr);removes++;
    if(remove_result==ERR_OK||remove_result==ERR_MEM||remove_result==ERR_ARG)fake_arp_found=false;
    return remove_result;
}
static pm_server_link_cache_result ram_read(void *ctx,uint32_t off,uint8_t *out,size_t n){
    assert(ctx==disk&&!lock_depth&&off<=sizeof disk&&n<=sizeof disk-off);memcpy(out,disk+off,n);return PM_SERVER_LINK_CACHE_OK;
}
static pm_server_link_cache_result ram_erase(void *ctx){
    assert(ctx==disk&&!lock_depth);memset(disk,255,sizeof disk);erases++;return PM_SERVER_LINK_CACHE_OK;
}
static pm_server_link_cache_result ram_program(void *ctx,uint32_t off,const uint8_t *page){
    assert(ctx==disk&&!lock_depth&&off%256==0&&off<=sizeof disk-256);
    for(unsigned i=0;i<256;i++){assert(disk[off+i]==255);disk[off+i]=page[i];}
    programs++;return PM_SERVER_LINK_CACHE_OK;
}
static const pm_server_link_cache_io io={disk,ram_read,ram_erase,ram_program};
static pm_server_link_cache_record record(pm_profile_id id,uint8_t last){
    pm_server_link_cache_record r={.profile=id,.ssid_length=4,.server_ipv4=original.server_ipv4,.prefix_length=24};
    memcpy(r.ssid,"test",4);memcpy(r.mac,original.mac,6);r.mac[5]=last;return r;
}
static void learn(pm_profile_id id,uint8_t last){
    pm_server_link_cache_record r=record(id,last);
    pm_server_link_cache_result result=pm_server_link_cache_remember(&r);
    assert(result==PM_SERVER_LINK_CACHE_CHANGED||result==PM_SERVER_LINK_CACHE_UNCHANGED);
}
static void reset(bool cached){
    remove_result=ERR_OK;pm_net_close();assert(!server_link.owned);
    memset(cache_blocked,0,sizeof cache_blocked);memset(&server_link,0,sizeof server_link);
    fail_alloc=fail_connect=false;add_result=remove_result=write_result=output_result=0;bad_readback=expect_seed=false;
    adds=removes=reads=programs=erases=0;event_tick=add_tick=readback_tick=alloc_tick=connect_tick=0;now_us=1000000;
    fake_arp_found=fake_ipv6=false;route=(struct netif){.up=true,.link_up=true,.num=1};fake_route=&route;
    assert(ipaddr_aton("192.168.0.44",&route.ip));assert(ipaddr_aton("255.255.255.0",&route.mask));
    memset(disk,255,sizeof disk);assert(pm_server_link_cache_load(&io)==PM_SERVER_LINK_CACHE_EMPTY);
    if(cached){learn(PM_PROFILE_HOME,7);learn(PM_PROFILE_HOME2,8);assert(pm_server_link_cache_flush(true,true)==PM_SERVER_LINK_CACHE_OK);assert(programs==2&&!erases);assert(pm_server_link_cache_load(&io)==PM_SERVER_LINK_CACHE_OK);}
    programs=erases=0;
}
static void begin(const pm_profile *p,pm_profile_id id,bool seeded){
    event_tick=add_tick=readback_tick=alloc_tick=connect_tick=0;expect_seed=seeded;connecting_profile=p;
    pm_net_begin_profile(p,id);expect_seed=false;
    assert(stage==PM_NET_TCP&&pcb);assert(server_link.owned==seeded&&server_link.provisional==seeded);
    pm_net_poll();assert(stage==PM_NET_TCP&&server_link.owned==seeded);
}
static void upgrade_ready(void){
    bool owned=server_link.owned;unsigned before=removes;
    cyw43_arch_lwip_begin();assert(on_connect(NULL,pcb,ERR_OK)==ERR_OK);cyw43_arch_lwip_end();
    assert(stage==PM_NET_WS&&server_link.owned==owned&&removes==before);pm_net_poll();
    assert(stage==PM_NET_WS&&server_link.owned==owned&&removes==before);
    char reply[256];int n=snprintf(reply,sizeof reply,"HTTP/1.1 101 Switching Protocols\r\nSec-WebSocket-Accept: %s\r\n\r\n",expected_accept);
    assert(n>0&&n<(int)sizeof reply);feed(reply,(unsigned)n);
    assert(stage==PM_NET_READY&&server_link.owned==owned&&removes==before);
}
static bool welcome(uint8_t last){
    pm_server_link_scope q;pm_net_server_link_query(&q);pm_server_link_hint h=original;
    h.client_ipv4=pm_server_link_ipv4((const uint8_t *)&route.ip);h.mac[5]=last;
    return pm_net_server_link_apply(&q,&h,3,100);
}
static void cached_ready(pm_profile_id id){reset(true);begin(&direct,id,true);upgrade_ready();}
static void case_bootstrap(void){
    for(unsigned p=0;p<2;p++){
        cached_ready(p?PM_PROFILE_HOME2:PM_PROFILE_HOME);uint8_t last=p?8:7;
        assert(adds==1&&server_link.provisional&&!server_link.attempted);assert(welcome(last));
        assert(!server_link.provisional&&server_link.seeded&&server_link.owned&&adds==2);
        assert(server_link.incarnation==3&&server_link.session==100);assert(!welcome(last)&&adds==2);
        pm_server_link_cache_record r=record(p?PM_PROFILE_HOME2:PM_PROFILE_HOME,last);
        assert(pm_server_link_cache_remember(&r)==PM_SERVER_LINK_CACHE_UNCHANGED&&!pm_server_link_cache_dirty());
        pm_net_close();assert(!server_link.owned&&removes==1&&!cache_blocked[p].valid);
    }
}
static void case_replacement(void){
    cached_ready(PM_PROFILE_HOME);assert(welcome(9));assert(fake_arp_mac.addr[5]==9&&server_link.mac[5]==9);
    /* Application submits accepted hint to RAM; network callbacks never flash. */
    learn(PM_PROFILE_HOME,9);assert(pm_server_link_cache_dirty()&&!programs&&!erases);
    pm_server_link_cache_record r;assert(pm_net_server_link_remembered(&r)&&r.mac[5]==9);
    assert(server_link.seed.mac[5]==7);pm_net_close();assert(!cache_blocked[0].valid);
    begin(&direct,PM_PROFILE_HOME,true);assert(server_link.mac[5]==9);
}
static void case_no_hint(void){
    for(unsigned invalid=0;invalid<2;invalid++){
        cached_ready(PM_PROFILE_HOME);
        if(invalid){pm_server_link_scope q;pm_net_server_link_query(&q);pm_server_link_hint bad=original;bad.mac[0]=1;assert(!pm_net_server_link_apply(&q,&bad,3,100));}
        assert(pm_net_server_link_without_hint()&&!server_link.owned&&!server_link.provisional&&removes==1);
        assert(stage==PM_NET_READY&&!pm_server_link_cache_dirty());assert(pm_net_server_link_without_hint()&&removes==1);
        assert(!welcome(9));pm_net_close();assert(!cache_blocked[0].valid);
        begin(&direct,PM_PROFILE_HOME,true);
    }
    cached_ready(PM_PROFILE_HOME);assert(welcome(7));unsigned before=removes;
    assert(!pm_net_server_link_without_hint());assert(server_link.owned&&removes==before);
}
static void case_lifetime(void){
    cached_ready(PM_PROFILE_HOME);assert(welcome(7));
    socket_instance.lastack=1;socket_instance.snd_lbb=10000;socket_instance.unacked=&socket_instance;
    for(unsigned i=0;i<1000;i++){now_us+=UINT64_C(3600000000);pm_net_poll();assert(server_link.owned);}
    ipaddr_aton("192.168.0.44",&route.ip);ipaddr_aton("255.255.255.0",&route.mask);pm_net_poll();
    assert(!removes&&!cache_blocked[0].valid&&!programs&&!erases);
    pm_net_close();begin(&direct,PM_PROFILE_HOME,true);
}
static void case_scope(void){
    for(unsigned phase=0;phase<2;phase++)for(unsigned mutation=0;mutation<10;mutation++){
        reset(true);begin(&direct,PM_PROFILE_HOME,true);if(phase){upgrade_ready();assert(welcome(7));}
        unsigned before=removes;
        switch(mutation){
            case 0:route.up=false;break;case 1:route.link_up=false;break;
            case 2:route.ip.addr++;break;case 3:route.mask.addr=0;break;
            case 4:fake_route=NULL;break;case 5:other_route=route;fake_route=&other_route;break;
            case 6:route.num++;break;case 7:socket_instance.remote_ip.addr++;break;
            case 8:fake_arp_found=false;remove_result=ERR_MEM;break;
            case 9:fake_arp_mac.addr[5]++;break;
        }
        pm_net_poll();assert(!server_link.owned&&removes==before+1&&cache_blocked[0].valid);
    }
    cached_ready(PM_PROFILE_HOME);pm_server_link_scope q;pm_net_server_link_query(&q);
    pm_net_close();begin(&direct,PM_PROFILE_HOME,false);upgrade_ready();
    assert(!pm_net_server_link_apply(&q,&original,3,100));
    cached_ready(PM_PROFILE_HOME);pm_net_server_link_query(&q);route.num++;
    assert(!pm_net_server_link_apply(&q,&original,3,100)&&!server_link.owned);
}
static void case_eligibility(void){
    for(unsigned i=0;i<12;i++){
        reset(true);pm_profile p=direct;pm_profile_id id=PM_PROFILE_HOME;
        switch(i){
            case 0:id=PM_PROFILE_AWAY;break;case 1:id=PM_PROFILE_COUNT;break;
            case 2:p=socks;break;case 3:p.ssid="changed";break;case 4:p.server_ip="192.168.0.121";break;
            case 5:route.up=false;break;case 6:route.link_up=false;break;
            case 7:route.ip.addr=0;break;case 8:ipaddr_aton("192.168.1.44",&route.ip);break;
            case 9:ipaddr_aton("255.255.0.0",&route.mask);break;case 10:fake_route=NULL;break;
            case 11:ipaddr_aton("255.0.255.0",&route.mask);break;
        }
        begin(&p,id,false);assert(!adds&&!removes&&!programs&&!erases);
    }
    reset(true);ipaddr_aton("192.168.0.45",&route.ip);begin(&direct,PM_PROFILE_HOME,true);upgrade_ready();assert(welcome(7));
}
static void case_cache_miss(void){
    reset(false);begin(&direct,PM_PROFILE_HOME,false);upgrade_ready();assert(!adds&&welcome(7));
    assert(server_link.owned&&!server_link.seeded&&!server_link.provisional);learn(PM_PROFILE_HOME,7);assert(pm_server_link_cache_dirty());
    pm_net_close();begin(&direct,PM_PROFILE_HOME,true);
}
static void case_install_failure(void){
    for(unsigned bad=0;bad<2;bad++){
        reset(true);if(bad)bad_readback=true;else add_result=ERR_MEM;
        begin(&direct,PM_PROFILE_HOME,false);assert(adds==1&&removes==bad&&cache_blocked[0].valid);
        bad_readback=false;add_result=ERR_OK;upgrade_ready();assert(welcome(7));
        pm_net_close();unsigned before=adds;begin(&direct,PM_PROFILE_HOME,false);assert(adds==before);
    }
    for(unsigned bad=0;bad<2;bad++){
        cached_ready(PM_PROFILE_HOME);if(bad)bad_readback=true;else add_result=ERR_MEM;
        assert(!welcome(9)&&!server_link.owned&&removes==1);assert(pm_net_server_link_without_hint());
        assert(stage==PM_NET_READY&&!pm_server_link_cache_dirty());
    }
}
static void case_cleanup_failure(void){
    cached_ready(PM_PROFILE_HOME);remove_result=ERR_RTE;unsigned before=creates;
    assert(!pm_net_server_link_without_hint()&&server_link.owned&&stage==PM_NET_FAILED);
    connecting_profile=&direct;pm_net_begin_profile(&direct,PM_PROFILE_HOME2);
    assert(server_link.owned&&creates==before&&stage==PM_NET_FAILED);
    remove_result=ERR_OK;begin(&direct,PM_PROFILE_HOME2,true);
    reset(true);bad_readback=true;remove_result=ERR_RTE;connecting_profile=&direct;before=creates;
    pm_net_begin_profile(&direct,PM_PROFILE_HOME);assert(stage==PM_NET_FAILED&&creates==before&&server_link.owned);
    pm_net_begin_profile(&direct,PM_PROFILE_HOME2);assert(stage==PM_NET_FAILED&&creates==before&&server_link.owned);
}
static void case_deadline_failure(void){
    for(unsigned phase=0;phase<3;phase++){
        reset(true);begin(&direct,PM_PROFILE_HOME,true);
        if(phase){cyw43_arch_lwip_begin();assert(on_connect(NULL,pcb,ERR_OK)==ERR_OK);cyw43_arch_lwip_end();}
        if(phase==2){char reply[256];int n=snprintf(reply,sizeof reply,"HTTP/1.1 101 Switching Protocols\r\nSec-WebSocket-Accept: %s\r\n\r\n",expected_accept);feed(reply,(unsigned)n);assert(stage==PM_NET_READY);}
        /* Public app deadline hook must precede close (main ordering is separate). */
        pm_net_server_link_connection_failed();pm_net_close();assert(cache_blocked[0].valid);
        begin(&direct,PM_PROFILE_HOME2,true);upgrade_ready();assert(welcome(8));pm_net_close();
        pm_net_close();begin(&direct,PM_PROFILE_HOME,false);upgrade_ready();
        pm_server_link_cache_record r;assert(pm_net_server_link_remembered(&r)&&r.mac[5]==7);
    }
}
static void case_terminal_failure(void){
    for(unsigned kind=0;kind<6;kind++){
        cached_ready(PM_PROFILE_HOME);assert(welcome(7));
        if(kind==0){cyw43_arch_lwip_begin();on_error(NULL,-14);cyw43_arch_lwip_end();}
        if(kind==1){write_result=ERR_MEM;assert(!pm_net_send(5,"{}"));write_result=0;}
        if(kind==2){stage=PM_NET_FAILED;}
        if(kind==3){pm_net_send_expired(tcp_status.connection_id);}
        if(kind==4){cyw43_arch_lwip_begin();assert(on_connect(NULL,pcb,-5)==-5);cyw43_arch_lwip_end();}
        if(kind==5){pm_net_server_link_connection_failed();}
        pm_net_poll();pm_net_close();assert(!server_link.owned&&cache_blocked[0].valid);
        begin(&direct,PM_PROFILE_HOME,false);
    }
    for(unsigned failure=0;failure<2;failure++){
        reset(true);fail_alloc=!failure;fail_connect=failure;connecting_profile=&direct;
        pm_net_begin_profile(&direct,PM_PROFILE_HOME);assert(stage==PM_NET_FAILED);
        pm_net_poll();pm_net_close();assert(cache_blocked[0].valid);
        fail_alloc=fail_connect=false;begin(&direct,PM_PROFILE_HOME,false);
    }
}
static void case_seed_identity(void){
    cached_ready(PM_PROFILE_HOME);assert(welcome(9));learn(PM_PROFILE_HOME,9);
    pm_net_server_link_connection_failed();pm_net_close();
    assert(cache_blocked[0].valid&&!cache_blocked[0].all&&cache_blocked[0].record.mac[5]==7);
    begin(&direct,PM_PROFILE_HOME,true);assert(server_link.seed.mac[5]==9);upgrade_ready();assert(welcome(10));learn(PM_PROFILE_HOME,10);
    pm_net_server_link_connection_failed();pm_net_close();assert(cache_blocked[0].all);
    begin(&direct,PM_PROFILE_HOME,false);upgrade_ready();pm_server_link_cache_record r;assert(pm_net_server_link_remembered(&r)&&r.mac[5]==10);
    pm_net_close();begin(&direct,PM_PROFILE_HOME2,true);assert(server_link.seed.mac[5]==8);
}
static void case_cancellation(void){
    reset(true);begin(&direct,PM_PROFILE_HOME,true);pm_net_close();assert(cache_blocked[0].valid);
    begin(&direct,PM_PROFILE_HOME,false);
    cached_ready(PM_PROFILE_HOME);assert(welcome(7));pm_net_close();assert(!cache_blocked[0].valid);
    begin(&direct,PM_PROFILE_HOME,true);
}
int main(int argc,char **argv){
    struct {const char *name;void(*test)(void);} tests[]={
        {"bootstrap",case_bootstrap},{"replacement",case_replacement},{"no-hint",case_no_hint},
        {"lifetime",case_lifetime},{"scope",case_scope},{"eligibility",case_eligibility},
        {"cache-miss",case_cache_miss},{"install-failure",case_install_failure},{"cleanup-failure",case_cleanup_failure},
        {"deadline-hook",case_deadline_failure},{"terminal-failure",case_terminal_failure},
        {"seed-identity",case_seed_identity},{"cancellation",case_cancellation}};
    assert(argc==2);
    for(unsigned i=0;i<sizeof tests/sizeof tests[0];i++)if(!strcmp(argv[1],tests[i].name)){
        tests[i].test();remove_result=ERR_OK;pm_net_close();assert(!lock_depth&&!programs&&!erases);
        printf("PASS production remembered owner: %s\n",tests[i].name);return 0;
    }
    fprintf(stderr,"Unknown case: %s\n",argv[1]);return 2;
}

#endif
