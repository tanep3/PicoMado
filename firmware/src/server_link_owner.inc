/* Network aggregate implementation: all effects run under its CYW43 lock. */
#if !ETHARP_SUPPORT_STATIC_ENTRIES
#error "Server link hint requires ETHARP_SUPPORT_STATIC_ENTRIES"
#endif
static struct {
    bool owned,attempted,remove_error_logged;
#if PM_SERVER_LINK_CACHE
    bool provisional,seeded;
    pm_server_link_cache_record seed;
#endif
    uint64_t generation,incarnation,session;
    ip4_addr_t address;pm_server_link_scope scope;uint8_t mac[6];
} server_link;
static pm_profile_id server_link_profile=PM_PROFILE_COUNT;
#if PM_SERVER_LINK_CACHE
/* A failed exact seed stays blocked across re-JOIN/profile changes this boot.
 * After a second different failed value, conservatively block that profile. */
static struct {bool valid,all;pm_server_link_cache_record record;} cache_blocked[2];
static unsigned cache_profile_slot(pm_profile_id id){return id==PM_PROFILE_HOME2?1u:0u;}
static bool cache_record_equal(const pm_server_link_cache_record *a,const pm_server_link_cache_record *b){
    return a->profile==b->profile&&a->ssid_length==b->ssid_length&&a->server_ipv4==b->server_ipv4&&
        a->prefix_length==b->prefix_length&&!memcmp(a->ssid,b->ssid,a->ssid_length)&&!memcmp(a->mac,b->mac,6);
}
static void server_link_seed_failed(void){
    if(!server_link.seeded)return;
    unsigned slot=cache_profile_slot(server_link.seed.profile);
    if(cache_blocked[slot].valid&&!cache_record_equal(&cache_blocked[slot].record,&server_link.seed))cache_blocked[slot].all=true;
    cache_blocked[slot].valid=true;cache_blocked[slot].record=server_link.seed;server_link.seeded=false;
    pm_logf("net server_cache ms=%lu conn=%llu event=blocked profile=%s\n",
        (unsigned long)(time_us_64()/1000),(unsigned long long)tcp_status.connection_id,pm_profile_name(server_link.seed.profile));
}
void pm_net_server_link_connection_failed(void){
    cyw43_arch_lwip_begin();server_link_seed_failed();cyw43_arch_lwip_end();
}
#endif
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
#if PM_SERVER_LINK_CACHE
    if(server_link.provisional&&!socket_dead&&!terminal_send_failed)
        s->ready=stage==PM_NET_TCP||stage==PM_NET_WS||stage==PM_NET_READY;
#endif
    s->local_ipv4=pm_server_link_ipv4((const uint8_t *)ip_2_ip4(&pcb->local_ip));
    s->peer_ipv4=pm_server_link_ipv4((const uint8_t *)ip_2_ip4(&pcb->remote_ip));
    struct netif *n=pcb->netif_idx!=NETIF_NO_INDEX?netif_get_by_index(pcb->netif_idx):ip4_route(ip_2_ip4(&pcb->remote_ip));
    if(!n)return;
    s->netif_identity=(uintptr_t)n;s->netif_index=netif_get_index(n);s->mask=pm_server_link_ipv4((const uint8_t *)netif_ip4_netmask(n));
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
    if(!found){
#if PM_SERVER_LINK_CACHE
        server_link_seed_failed();
#endif
        server_link_remove("link_scope");if(server_link.generation<UINT64_MAX)server_link.generation++;
    }
}
static void server_link_close(void){
#if PM_SERVER_LINK_CACHE
    if(server_link.provisional||socket_dead||terminal_send_failed||stage==PM_NET_FAILED)server_link_seed_failed();
#endif
    server_link_remove("close");server_link.attempted=false;
    if(server_link.generation<UINT64_MAX)server_link.generation++;
    server_link_profile=PM_PROFILE_COUNT;
#if PM_SERVER_LINK_CACHE
    server_link.provisional=false;server_link.seeded=false;
#endif
}
void pm_net_server_link_query(pm_server_link_scope *out){
    if(!out)return;
    cyw43_arch_lwip_begin();server_link_maintain();server_link_observe(out);cyw43_arch_lwip_end();
}
bool pm_net_server_link_apply(const pm_server_link_scope *query,const pm_server_link_hint *hint,uint64_t incarnation,uint64_t session){
    if(!query||!hint)return false;
    cyw43_arch_lwip_begin();server_link_maintain();pm_server_link_scope now;server_link_observe(&now);
    bool accepted=false;const char *event="rejected";err_t result=ERR_ARG;
    bool available=!server_link.owned;
#if PM_SERVER_LINK_CACHE
    available=available||server_link.provisional;
#endif
    if(available&&!server_link.attempted&&incarnation&&session&&stage==PM_NET_READY&&pm_server_link_admit(query,&now,hint)){
        server_link.attempted=true;server_link.scope=now;server_link.incarnation=incarnation;server_link.session=session;
        server_link.address=*ip_2_ip4(&pcb->remote_ip);struct eth_addr target;memcpy(target.addr,hint->mac,6);
        result=etharp_add_static_entry(&server_link.address,&target);event="add_failed";
        if(result==ERR_OK){
            server_link.owned=true;server_link.remove_error_logged=false;memcpy(server_link.mac,hint->mac,6);
            struct eth_addr *actual=NULL;const ip4_addr_t *resolved=NULL;
            if(etharp_find_addr((struct netif *)now.netif_identity,&server_link.address,&actual,&resolved)>=0 && actual && resolved &&
               resolved->addr==server_link.address.addr&&!memcmp(actual->addr,hint->mac,6)){
                accepted=true;event="installed";
#if PM_SERVER_LINK_CACHE
                server_link.provisional=false;
#endif
            }else {server_link_remove("readback");event="readback_failed";result=ERR_VAL;}
        }
#if PM_SERVER_LINK_CACHE
        else if(server_link.owned)server_link_remove("replace_failed");
#endif
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

#if PM_SERVER_LINK_CACHE
bool pm_net_server_link_remembered(pm_server_link_cache_record *out){
    if(!out)return false;
    cyw43_arch_lwip_begin();
    bool valid=profile&&profile->ssid&&!profile->socks&&stage==PM_NET_READY&&!socket_dead&&
        pm_server_link_cache_get(server_link_profile,(const uint8_t *)profile->ssid,strlen(profile->ssid),
            pm_server_link_ipv4((const uint8_t *)ip_2_ip4(&server_addr)),out);
    cyw43_arch_lwip_end();return valid;
}
bool pm_net_server_link_without_hint(void){
    cyw43_arch_lwip_begin();
    if(server_link.provisional){server_link_remove("no_current_hint");server_link.provisional=false;}
    bool clear=!server_link.owned;server_link.attempted=true;
    if(!clear)stage=PM_NET_FAILED;
    cyw43_arch_lwip_end();return clear;
}
/* Only called from main-owned begin, before allocating or connecting a PCB. */
static bool server_link_cache_prepare(void){
    if(server_link_profile!=PM_PROFILE_HOME&&server_link_profile!=PM_PROFILE_HOME2)return true;
    if(!profile||profile->socks||!profile->ssid)return true;
    pm_server_link_cache_record record;uint32_t target=pm_server_link_ipv4((const uint8_t *)ip_2_ip4(&server_addr));
    if(!pm_server_link_cache_get(server_link_profile,(const uint8_t *)profile->ssid,strlen(profile->ssid),target,&record))return true;
    unsigned slot=cache_profile_slot(server_link_profile);
    if(cache_blocked[slot].all||(cache_blocked[slot].valid&&cache_record_equal(&cache_blocked[slot].record,&record))){
        pm_logf("net server_cache ms=%lu conn=%llu event=fallback profile=%s\n",(unsigned long)(time_us_64()/1000),
            (unsigned long long)tcp_status.connection_id,pm_profile_name(server_link_profile));return true;
    }
    struct netif *n=ip4_route(ip_2_ip4(&server_addr));if(!n)return true;
    pm_server_link_scope scope={.connection=tcp_status.connection_id,.generation=server_link.generation,
        .profile=server_link_profile,.netif_identity=(uintptr_t)n,.netif_index=netif_get_index(n),
        .local_ipv4=pm_server_link_ipv4((const uint8_t *)netif_ip4_addr(n)),.peer_ipv4=target,
        .mask=pm_server_link_ipv4((const uint8_t *)netif_ip4_netmask(n)),.ready=true,
        .up=netif_is_up(n)&&netif_is_link_up(n),.direct=true};
    pm_server_link_hint hint={.server_ipv4=target,.client_ipv4=scope.local_ipv4,.prefix_length=record.prefix_length};
    memcpy(hint.mac,record.mac,6);
    if(!pm_server_link_admit(&scope,&scope,&hint))return true;
    server_link.seed=record;server_link.seeded=true;server_link.scope=scope;server_link.address=*ip_2_ip4(&server_addr);
    struct eth_addr mac;memcpy(mac.addr,record.mac,6);err_t result=etharp_add_static_entry(&server_link.address,&mac);
    const char *event="add_failed";
    if(result==ERR_OK){
        server_link.owned=true;server_link.remove_error_logged=false;memcpy(server_link.mac,record.mac,6);
        struct eth_addr *actual=NULL;const ip4_addr_t *resolved=NULL;
        if(etharp_find_addr(n,&server_link.address,&actual,&resolved)>=0&&actual&&resolved&&
           resolved->addr==server_link.address.addr&&!memcmp(actual->addr,record.mac,6)){
            server_link.provisional=true;event="installed_before_tcp";
        }else {result=ERR_VAL;event="readback_failed";server_link_remove("cache_readback");}
    }
    if(result!=ERR_OK)server_link_seed_failed();
    pm_logf("net server_cache ms=%lu conn=%llu profile=%s event=%s result=%d\n",(unsigned long)(time_us_64()/1000),
        (unsigned long long)tcp_status.connection_id,pm_profile_name(server_link_profile),event,(int)result);
    if(result==ERR_OK)pm_logf("net server_cache conn=%llu ip=%u.%u.%u.%u mac=%02x:%02x:%02x:%02x:%02x:%02x\n",
        (unsigned long long)tcp_status.connection_id,(unsigned)(target>>24),(unsigned)((target>>16)&255),
        (unsigned)((target>>8)&255),(unsigned)(target&255),record.mac[0],record.mac[1],record.mac[2],record.mac[3],record.mac[4],record.mac[5]);
    return result==ERR_OK||!server_link.owned;
}
#endif
