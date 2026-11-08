/* Included by the network aggregate, only in the explicit diagnostic build.
 * All calls run under its existing CYW43/lwIP lock. */
#if !ETHARP_SUPPORT_STATIC_ENTRIES
#error "HOME static-ARP diagnostic requires ETHARP_SUPPORT_STATIC_ENTRIES"
#endif
static bool home_arp_owned;
static ip4_addr_t home_arp_address;
static void home_arp_remove(void){
    if(!home_arp_owned)return;
    err_t result=etharp_remove_static_entry(&home_arp_address);
    /* SDK: OK=removed, MEM=absent, ARG=dynamic (not ours). Unknown failures
     * retain ownership and block another connection/profile below. */
    if(result==ERR_OK || result==ERR_MEM || result==ERR_ARG)home_arp_owned=false;
    pm_logf("net static_arp ms=%lu conn=%llu event=remove result=%d\n",
        (unsigned long)(time_us_64()/1000),(unsigned long long)tcp_status.connection_id,(int)result);
}
static bool home_arp_prepare(const pm_profile *p,pm_profile_id id){
    pm_home_arp_action action=pm_home_arp_scope(PM_DEVICE_ID,id,p->socks,p->server_ip);
    const char *event="scope_failed";
    err_t result=ERR_ARG;
    if(action==PM_HOME_ARP_SKIP){event="skip";result=ERR_OK;}
    else if(action==PM_HOME_ARP_PIN){
        struct netif *n=ip4_route(ip_2_ip4(&server_addr));
        event="route_failed";result=ERR_RTE;
        if(n && netif_is_up(n) && netif_is_link_up(n) &&
           !ip4_addr_isany_val(*netif_ip4_addr(n)) &&
           !ip4_addr_isany_val(*netif_ip4_netmask(n)) &&
           ip4_addr_netcmp(ip_2_ip4(&server_addr),netif_ip4_addr(n),netif_ip4_netmask(n))){
            struct eth_addr mac={{0x2c,0xcf,0x67,0x11,0x72,0xf3}};
            home_arp_address=*ip_2_ip4(&server_addr);
            result=etharp_add_static_entry(&home_arp_address,&mac);event="add_failed";
            if(result==ERR_OK){
                home_arp_owned=true;
                struct eth_addr *actual=NULL;const ip4_addr_t *resolved=NULL;
                if(etharp_find_addr(n,&home_arp_address,&actual,&resolved)>=0 && actual &&
                   resolved && resolved->addr==home_arp_address.addr && !memcmp(actual->addr,mac.addr,6)){
                    pm_logf("net static_arp ms=%lu conn=%llu profile=HOME event=fixed ip=" PM_HOME_STATIC_ARP_IP " mac=2c:cf:67:11:72:f3 result=0\n",
                        (unsigned long)(time_us_64()/1000),(unsigned long long)tcp_status.connection_id);
                    return true;
                }
                home_arp_remove();event="readback_failed";result=ERR_VAL;
            }
        }
    }
    pm_logf("net static_arp ms=%lu conn=%llu profile=%s event=%s result=%d\n",
        (unsigned long)(time_us_64()/1000),(unsigned long long)tcp_status.connection_id,
        pm_profile_name(id),event,(int)result);
    return action==PM_HOME_ARP_SKIP;
}
