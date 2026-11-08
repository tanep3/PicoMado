/* Production net.c with deterministic TCP/ARP IO; no actual network. */
#define main baseline_net_main
#include "net_nagle_test.c"
#undef main
static unsigned adds,removes,expected_creates;
static err_t add_result,remove_result;
static bool bad_readback,missing_readback;
static struct netif route;
static const pm_profile pinned={"test","test",PM_HOME_STATIC_ARP_IP,false,NULL,NULL,NULL,0};
err_t etharp_add_static_entry(const ip4_addr_t *ip,struct eth_addr *mac){
    assert(lock_depth==1 && !pcb && creates==expected_creates);
    ip_addr_t expected;assert(ipaddr_aton(PM_HOME_STATIC_ARP_IP,&expected));
    assert(ip->addr==expected.addr && !memcmp(mac->addr,(uint8_t[]){0x2c,0xcf,0x67,0x11,0x72,0xf3},6));
    adds++;if(add_result)return add_result;
    fake_arp_found=!missing_readback;fake_arp_mac=*mac;
    if(bad_readback)fake_arp_mac.addr[5]^=1;
    return ERR_OK;
}
err_t etharp_remove_static_entry(const ip4_addr_t *ip){
    assert(lock_depth==1 && !pcb && ip->addr==home_arp_address.addr);
    removes++;fake_arp_found=false;return remove_result;
}
static void reset(void){
    remove_result=0;
    pm_net_close();assert(!home_arp_owned);
    add_result=remove_result=0;bad_readback=missing_readback=false;
    fail_alloc=fail_connect=false;fake_route=&route;route.up=route.link_up=true;
    ipaddr_aton("192.168.0.44",&route.ip);ipaddr_aton("255.255.255.0",&route.mask);
    adds=removes=0;expected_creates=creates;connecting_profile=&pinned;
}
static void begin(pm_profile_id id){expected_creates=creates;pm_net_begin_home_static_arp(&pinned,id);assert(!lock_depth);}
int main(void){
    assert(pm_home_arp_scope("E-ink_1",PM_PROFILE_HOME,false,PM_HOME_STATIC_ARP_IP)==PM_HOME_ARP_PIN);
    assert(pm_home_arp_scope("E-ink_0",PM_PROFILE_HOME,false,PM_HOME_STATIC_ARP_IP)==PM_HOME_ARP_SKIP);
    assert(pm_home_arp_scope(NULL,PM_PROFILE_HOME,false,PM_HOME_STATIC_ARP_IP)==PM_HOME_ARP_SKIP);
    reset();begin(PM_PROFILE_HOME);assert(adds==1 && home_arp_owned && stage==PM_NET_TCP);
    begin(PM_PROFILE_HOME);assert(adds==2 && removes==1 && home_arp_owned);
    pm_net_close();pm_net_close();assert(removes==2 && !home_arp_owned);
    for(unsigned i=1;i<=PM_PROFILE_COUNT;i++){
        reset();begin(PM_PROFILE_HOME);begin((pm_profile_id)i);
        assert(adds==1 && removes==1 && !home_arp_owned && stage==PM_NET_TCP);
    }
    reset();pm_net_begin(&pinned);assert(!adds && !home_arp_owned && stage==PM_NET_TCP);
    for(unsigned i=0;i<5;i++){
        reset();unsigned count=creates;
        if(i==0)fake_route=NULL;
        if(i==1)route.up=false;
        if(i==2)route.link_up=false;
        if(i==3)ipaddr_aton("198.51.100.44",&route.ip);
        if(i==4)route.mask.addr=0;
        begin(PM_PROFILE_HOME);assert(!adds && creates==count && stage==PM_NET_FAILED);
    }
    reset();route.ip.addr=0;begin(PM_PROFILE_HOME);assert(!adds && stage==PM_NET_FAILED);
    reset();unsigned count=creates;add_result=ERR_MEM;begin(PM_PROFILE_HOME);
    assert(adds==1 && !removes && !home_arp_owned && creates==count && stage==PM_NET_FAILED);
    for(unsigned i=0;i<2;i++){
        reset();count=creates;bad_readback=i==0;missing_readback=i==1;begin(PM_PROFILE_HOME);
        assert(adds==1 && removes==1 && !home_arp_owned && creates==count && stage==PM_NET_FAILED);
    }
    reset();fail_alloc=true;begin(PM_PROFILE_HOME);assert(home_arp_owned && stage==PM_NET_FAILED);
    pm_net_close();assert(removes==1 && !home_arp_owned);
    reset();fail_connect=true;begin(PM_PROFILE_HOME);assert(home_arp_owned && stage==PM_NET_FAILED);
    pm_net_close();assert(removes==1 && !home_arp_owned);
    for(unsigned i=0;i<2;i++){
        reset();begin(PM_PROFILE_HOME);remove_result=i?ERR_ARG:ERR_MEM;
        pm_net_close();pm_net_close();assert(removes==1 && !home_arp_owned);
    }
    reset();begin(PM_PROFILE_HOME);remove_result=ERR_RTE;count=creates;
    begin(PM_PROFILE_HOME2);assert(home_arp_owned && creates==count && stage==PM_NET_FAILED);
    remove_result=ERR_OK;begin(PM_PROFILE_HOME2);assert(!home_arp_owned && stage==PM_NET_TCP);
    reset();pm_profile other=pinned;other.server_ip="192.168.0.121";
    pm_net_begin_home_static_arp(&other,PM_PROFILE_HOME);assert(!adds && stage==PM_NET_FAILED);
    other=away;pm_net_begin_home_static_arp(&other,PM_PROFILE_HOME);assert(!adds && stage==PM_NET_FAILED);
    pm_net_close();puts("PASS HOME static ARP: scope, pre-TCP lock/order, skip, fail-closed, readback, retry, close, SDK cleanup and allocation/connect errors");
}
