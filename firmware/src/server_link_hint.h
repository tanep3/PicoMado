#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include "network_profiles.h"
#ifndef PM_SERVER_LINK_HINT
#define PM_SERVER_LINK_HINT 0
#endif
#if PM_SERVER_LINK_HINT != 0 && PM_SERVER_LINK_HINT != 1
#error "PM_SERVER_LINK_HINT must be 0 or 1"
#endif
#define PM_SERVER_LINK_CAPABILITY "server-link-hint-v1"
#define PM_SERVER_LINK_WELCOME_MAX 1024u
/* IPv4 values are logical big-endian integers, independent of CPU byte order. */
typedef struct {uint32_t server_ipv4,client_ipv4;uint8_t prefix_length,mac[6];} pm_server_link_hint;
typedef enum {PM_SERVER_LINK_ABSENT,PM_SERVER_LINK_INVALID,PM_SERVER_LINK_VALID} pm_server_link_parse;
typedef struct {
    uint64_t incarnation,session_epoch,media_input_version;
    pm_server_link_parse link;pm_server_link_hint hint;
} pm_server_welcome;
typedef struct {
    uint64_t connection,generation;uintptr_t netif_identity;
    uint32_t local_ipv4,peer_ipv4,mask;pm_profile_id profile;uint8_t netif_index;
    bool ready,up,direct;
} pm_server_link_scope;
bool pm_server_welcome_parse(const char *json,size_t length,pm_server_welcome *out);
uint32_t pm_server_link_ipv4(const uint8_t bytes[4]);
unsigned pm_server_link_prefix(uint32_t mask);
bool pm_server_link_scope_equal(const pm_server_link_scope *a,const pm_server_link_scope *b);
bool pm_server_link_admit(const pm_server_link_scope *query,const pm_server_link_scope *current,const pm_server_link_hint *hint);
