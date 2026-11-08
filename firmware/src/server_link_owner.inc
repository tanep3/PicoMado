/* Network aggregate implementation: all effects run under its CYW43 lock. */
#if !ETHARP_SUPPORT_STATIC_ENTRIES
#error "Server link hint requires ETHARP_SUPPORT_STATIC_ENTRIES"
#endif
static struct {
    bool owned,attempted,remove_error_logged;
    uint64_t generation,incarnation,session;
    ip4_addr_t address;pm_server_link_scope scope;uint8_t mac[6];
} server_link;
static pm_profile_id server_link_profile=PM_PROFILE_COUNT;
static void server_link_remove(const char *reason){
    if(!server_link.owned)return;
    err_t result=etharp_remove_static_entry(&server_link.address);
    bool released=result==ERR_OK||result==ERR_MEM||result==ERR_ARG;
    if(released || !server_link.remove_error_logged){
        pm_logf("net server_link ms=%lu conn=%llu event=remove reason=%s result=%d\n",
            (unsigned long)(time_us_64()/1000),(unsigned long long)server_link.scope.connection,reason,(int)result);
    }
    server_link.owned=!released;server_link.remove_error_logged=!released;
}
static void server_link_observe(pm_server_link_scope *s){
    *s=(pm_server_link_scope){.connection=tcp_status.connection_id,.generation=server_link.generation,.profile=server_link_profile};
    if(!pcb||!profile||!IP_IS_V4(&pcb->remote_ip))return;
    s->ready=stage==PM_NET_READY&&!socket_dead&&!terminal_send_failed;
    s->local_ipv4=pm_server_link_ipv4((const uint8_t *)ip_2_ip4(&pcb->local_ip));
    s->peer_ipv4=pm_server_link_ipv4((const uint8_t *)ip_2_ip4(&pcb->remote_ip));
    struct netif *n=pcb->netif_idx!=NETIF_NO_INDEX?netif_get_by_index(pcb->netif_idx):ip4_route(ip_2_ip4(&pcb->remote_ip));
    if(!n)return;
    s->netif_identity=(uintptr_t)n;s->mask=pm_server_link_ipv4((const uint8_t *)netif_ip4_netmask(n));
    s->up=netif_is_up(n)&&netif_is_link_up(n)&&s->local_ipv4==pm_server_link_ipv4((const uint8_t *)netif_ip4_addr(n));
    s->direct=!profile->socks&&s->peer_ipv4==pm_server_link_ipv4((const uint8_t *)ip_2_ip4(&server_addr))&&pm_server_link_prefix(s->mask)>0 &&
        (s->peer_ipv4&s->mask)==(s->local_ipv4&s->mask) &&
        ip4_route(ip_2_ip4(&pcb->remote_ip))==n;
}
static void server_link_maintain(void){
    if(!server_link.owned)return;
    pm_server_link_scope now;server_link_observe(&now);
    struct eth_addr *mac=NULL;const ip4_addr_t *resolved=NULL;
    bool unchanged=pm_server_link_scope_equal(&server_link.scope,&now) && now.ready&&now.up&&now.direct;
    bool found=unchanged && etharp_find_addr((struct netif *)now.netif_identity,&server_link.address,&mac,&resolved)>=0 &&
        mac&&resolved&&resolved->addr==server_link.address.addr&&!memcmp(mac->addr,server_link.mac,6);
    if(!found){server_link_remove("link_scope");if(server_link.generation<UINT64_MAX)server_link.generation++;}
}
static void server_link_close(void){
    server_link_remove("close");server_link.attempted=false;
    if(server_link.generation<UINT64_MAX)server_link.generation++;
    server_link_profile=PM_PROFILE_COUNT;
}
void pm_net_server_link_query(pm_server_link_scope *out){
    if(!out)return;
    cyw43_arch_lwip_begin();server_link_maintain();server_link_observe(out);cyw43_arch_lwip_end();
}
bool pm_net_server_link_apply(const pm_server_link_scope *query,const pm_server_link_hint *hint,uint64_t incarnation,uint64_t session){
    if(!query||!hint)return false;
    cyw43_arch_lwip_begin();server_link_maintain();pm_server_link_scope now;server_link_observe(&now);
    bool accepted=false;const char *event="rejected";err_t result=ERR_ARG;
    if(!server_link.owned&&!server_link.attempted&&incarnation&&session&&pm_server_link_admit(query,&now,hint)){
        server_link.attempted=true;server_link.scope=now;server_link.incarnation=incarnation;server_link.session=session;
        server_link.address=*ip_2_ip4(&pcb->remote_ip);struct eth_addr target;memcpy(target.addr,hint->mac,6);
        result=etharp_add_static_entry(&server_link.address,&target);event="add_failed";
        if(result==ERR_OK){
            server_link.owned=true;server_link.remove_error_logged=false;memcpy(server_link.mac,hint->mac,6);
            struct eth_addr *actual=NULL;const ip4_addr_t *resolved=NULL;
            if(etharp_find_addr((struct netif *)now.netif_identity,&server_link.address,&actual,&resolved)>=0 && actual && resolved &&
               resolved->addr==server_link.address.addr&&!memcmp(actual->addr,hint->mac,6)){
                accepted=true;event="installed";
            }else {server_link_remove("readback");event="readback_failed";result=ERR_VAL;}
        }
    }
    pm_logf("net server_link ms=%lu conn=%llu profile=%s event=%s result=%d\n",
        (unsigned long)(time_us_64()/1000),(unsigned long long)now.connection,pm_profile_name(now.profile),event,(int)result);
    if(accepted){
        pm_logf("net server_link conn=%llu incarnation=%llu session=%llu\n",(unsigned long long)now.connection,(unsigned long long)incarnation,(unsigned long long)session);
        pm_logf("net server_link conn=%llu ip=%u.%u.%u.%u mac=%02x:%02x:%02x:%02x:%02x:%02x\n",
            (unsigned long long)now.connection,(unsigned)(hint->server_ipv4>>24),(unsigned)((hint->server_ipv4>>16)&255),
            (unsigned)((hint->server_ipv4>>8)&255),(unsigned)(hint->server_ipv4&255),hint->mac[0],hint->mac[1],hint->mac[2],hint->mac[3],hint->mac[4],hint->mac[5]);
    }
    cyw43_arch_lwip_end();return accepted;
}
