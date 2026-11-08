#define main baseline_net_main
#include "net_nagle_test.c"
#undef main
static unsigned adds,removes;
static err_t add_result,remove_result;
static bool bad_readback;
static struct netif route;
static const pm_profile direct_home={"test","test","192.168.0.120",false,NULL,NULL,NULL,0};
static pm_server_link_hint hint={.server_ipv4=0xc0a80078,.client_ipv4=0xc0a8002c,.prefix_length=24,.mac={0x2c,0xcf,0x67,0x11,0x72,0xf3}};
err_t etharp_add_static_entry(const ip4_addr_t *ip,struct eth_addr *mac){
    assert(lock_depth==1&&pcb&&stage==PM_NET_READY&&ip->addr==pcb->remote_ip.addr);adds++;
    if(add_result)return add_result;
    fake_arp_found=true;fake_arp_mac=*mac;if(bad_readback)fake_arp_mac.addr[5]^=1;return ERR_OK;
}
err_t etharp_remove_static_entry(const ip4_addr_t *ip){
    assert(lock_depth==1&&ip->addr==server_link.address.addr);removes++;
    if(remove_result==ERR_OK||remove_result==ERR_MEM||remove_result==ERR_ARG)fake_arp_found=false;
    return remove_result;
}
static void fresh(pm_profile_id id){
    remove_result=0;pm_net_close();assert(!server_link.owned);adds=removes=0;add_result=0;bad_readback=false;
    fake_route=&route;route.up=route.link_up=true;ipaddr_aton("192.168.0.44",&route.ip);ipaddr_aton("255.255.255.0",&route.mask);
    connecting_profile=&direct_home;pm_net_begin_profile(&direct_home,id);assert(pcb);stage=PM_NET_READY;
    socket_instance.local_ip=route.ip;ipaddr_aton(direct_home.server_ip,&socket_instance.remote_ip);
}
static bool apply(void){pm_server_link_scope q;pm_net_server_link_query(&q);return pm_net_server_link_apply(&q,&hint,3,100);}
int main(void){
    for(unsigned i=0;i<2;i++){
        fresh(i?PM_PROFILE_HOME2:PM_PROFILE_HOME);assert(apply()&&server_link.owned&&adds==1);
        assert(server_link.incarnation==3&&server_link.session==100);assert(!apply()&&adds==1);
        // Neither queued/unacknowledged data nor polling acts as TTL expiry.
        socket_instance.lastack=1;socket_instance.snd_lbb=100;socket_instance.unacked=&socket_instance;
        for(unsigned n=0;n<1000;n++)pm_net_poll();
        assert(server_link.owned&&!removes);
        pm_net_close();pm_net_close();assert(!server_link.owned&&removes==1);
    }
    fresh(PM_PROFILE_AWAY);assert(!apply()&&!adds);fresh(PM_PROFILE_COUNT);assert(!apply()&&!adds);
    fresh(PM_PROFILE_HOME);pm_server_link_scope old;pm_net_server_link_query(&old);fresh(PM_PROFILE_HOME);
    assert(!pm_net_server_link_apply(&old,&hint,3,100)&&!adds);assert(apply());
    for(unsigned i=0;i<5;i++){
        fresh(PM_PROFILE_HOME);assert(apply());
        if(i==0)route.up=false;
        if(i==1)route.link_up=false;
        if(i==2)route.ip.addr++;
        if(i==3)route.mask.addr=0;
        if(i==4)fake_route=NULL;
        pm_net_poll();assert(!server_link.owned&&removes==1);
    }
    fresh(PM_PROFILE_HOME);assert(apply());fake_arp_found=false;remove_result=ERR_MEM;pm_net_poll();assert(!server_link.owned);
    fresh(PM_PROFILE_HOME);assert(apply());struct netif replacement=route;fake_route=&replacement;
    pm_net_poll();assert(!server_link.owned&&removes==1);
    fresh(PM_PROFILE_HOME);assert(apply());ipaddr_aton("192.168.0.44",&route.ip);
    ipaddr_aton("255.255.255.0",&route.mask);pm_net_poll();assert(server_link.owned&&!removes);
    fresh(PM_PROFILE_HOME);add_result=ERR_MEM;assert(!apply()&&!server_link.owned&&stage==PM_NET_READY&&adds==1);add_result=0;assert(!apply()&&adds==1);
    fresh(PM_PROFILE_HOME);bad_readback=true;assert(!apply()&&!server_link.owned&&removes==1&&stage==PM_NET_READY);
    fresh(PM_PROFILE_HOME);assert(apply());remove_result=ERR_RTE;unsigned count=creates;
    pm_net_begin_profile(&direct_home,PM_PROFILE_HOME2);assert(server_link.owned&&creates==count&&stage==PM_NET_FAILED);
    remove_result=ERR_OK;fresh(PM_PROFILE_HOME2);assert(apply());
    // SDK-owned PCB error and existing terminal cleanup release the mapping.
    cyw43_arch_lwip_begin();on_error(NULL,-14);cyw43_arch_lwip_end();pm_net_poll();assert(!server_link.owned);
    fresh(PM_PROFILE_HOME);assert(apply());write_result=ERR_MEM;assert(!pm_net_send(5,"{}"));write_result=0;
    assert(pm_net_checkpoint()==PM_NET_FAILED&&!server_link.owned);
    fresh(PM_PROFILE_HOME);pm_net_server_link_query(&old);route.mask.addr=0;
    assert(!pm_net_server_link_apply(&old,&hint,3,100)&&!adds);
    pm_net_close();assert(!lock_depth);
    puts("PASS production link owner: locked post-Welcome pin, HOME/HOME2, generation/profile/link fences, no ACK/TTL reset, retry/close/terminal/SDK cleanup and failures");
}
