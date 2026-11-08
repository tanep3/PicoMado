#pragma once
#ifndef PM_HOME_STATIC_ARP_DIAGNOSTIC
#define PM_HOME_STATIC_ARP_DIAGNOSTIC 0
#endif
#if PM_HOME_STATIC_ARP_DIAGNOSTIC != 0 && PM_HOME_STATIC_ARP_DIAGNOSTIC != 1
#error "PM_HOME_STATIC_ARP_DIAGNOSTIC must be 0 or 1"
#endif
#if PM_HOME_STATIC_ARP_DIAGNOSTIC
#include <stdbool.h>
#include <string.h>
#include "network_profiles.h"
#define PM_HOME_STATIC_ARP_IP "192.168.0.120"
typedef enum { PM_HOME_ARP_SKIP, PM_HOME_ARP_PIN, PM_HOME_ARP_REJECT } pm_home_arp_action;
/* Pure scope decision. Network/ARP effects belong exclusively to net.c. */
static inline pm_home_arp_action pm_home_arp_scope(const char *device,
        pm_profile_id id,bool socks,const char *server){
    if(!device || strcmp(device,"E-ink_1") || id!=PM_PROFILE_HOME)return PM_HOME_ARP_SKIP;
    return !socks && server && !strcmp(server,PM_HOME_STATIC_ARP_IP)?PM_HOME_ARP_PIN:PM_HOME_ARP_REJECT;
}
#endif
