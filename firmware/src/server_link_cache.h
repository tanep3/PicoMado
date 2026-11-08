#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include "network_profiles.h"

#ifndef PM_SERVER_LINK_HINT
#define PM_SERVER_LINK_HINT 0
#endif
#ifndef PM_SERVER_LINK_CACHE
#define PM_SERVER_LINK_CACHE 0
#endif
#if PM_SERVER_LINK_CACHE != 0 && PM_SERVER_LINK_CACHE != 1
#error "PM_SERVER_LINK_CACHE must be 0 or 1"
#endif
#if PM_SERVER_LINK_CACHE && !PM_SERVER_LINK_HINT
#error "PM_SERVER_LINK_CACHE requires PM_SERVER_LINK_HINT"
#endif
#define PM_SERVER_LINK_CACHE_CAPABILITY "server-link-cache-v1"
#define PM_SERVER_LINK_CACHE_SECTOR_SIZE 4096u
#define PM_SERVER_LINK_CACHE_PAGE_SIZE 256u
#define PM_SERVER_LINK_CACHE_RESERVED_TAIL 16384u
#define PM_SERVER_LINK_CACHE_SSID_MAX 32u
/* IPv4 is a logical big-endian integer, not CPU/network memory representation. */
typedef struct {
    pm_profile_id profile;
    uint8_t ssid_length, ssid[PM_SERVER_LINK_CACHE_SSID_MAX];
    uint32_t server_ipv4;
    uint8_t prefix_length, mac[6];
} pm_server_link_cache_record;

typedef enum {
    PM_SERVER_LINK_CACHE_OK, PM_SERVER_LINK_CACHE_EMPTY,
    PM_SERVER_LINK_CACHE_UNCHANGED, PM_SERVER_LINK_CACHE_CHANGED,
    PM_SERVER_LINK_CACHE_INVALID, PM_SERVER_LINK_CACHE_DEFERRED,
    PM_SERVER_LINK_CACHE_NEEDS_ERASE, PM_SERVER_LINK_CACHE_SEQUENCE_EXHAUSTED,
    PM_SERVER_LINK_CACHE_READ_FAILED, PM_SERVER_LINK_CACHE_SAFETY_FAILED,
    PM_SERVER_LINK_CACHE_ERASE_FAILED, PM_SERVER_LINK_CACHE_PROGRAM_FAILED,
    PM_SERVER_LINK_CACHE_READBACK_FAILED, PM_SERVER_LINK_CACHE_COMMIT_FAILED
} pm_server_link_cache_result;

/* Offsets are sector-relative. Adapters must confine effects to this one sector.
 * A write error may have executed partially: the owner rescans before retrying.
 * Return OK only on success; read/erase/program failures have distinct results.
 */
typedef struct {
    void *context;
    pm_server_link_cache_result (*read)(void *, uint32_t, uint8_t *, size_t);
    pm_server_link_cache_result (*erase)(void *);
    pm_server_link_cache_result (*program)(void *, uint32_t, const uint8_t *);
} pm_server_link_cache_io;

/* Single main-loop owner. No API is IRQ-safe or callable inside lwIP callbacks.
 * Scratch is static and exclusively owned by an active load/needs_erase/flush.
 * Synchronous IO callback re-entry never changes state, bindings, or outputs:
 * load/remember/flush return DEFERRED, get returns false, needs_erase returns
 * true conservatively. dirty is observation-only; record_valid/result_name
 * remain pure. This recursion guard is not an IRQ/multicore synchronization API.
 * An admitted load resets RAM, never writes, and copies the IO table. Its
 * failure leaves an empty cache; a deferred recursive load leaves it unchanged.
 */
pm_server_link_cache_result pm_server_link_cache_load(const pm_server_link_cache_io *io);
bool pm_server_link_cache_record_valid(const pm_server_link_cache_record *record);
bool pm_server_link_cache_get(pm_profile_id profile, const uint8_t *ssid,
    size_t ssid_length, uint32_t server_ipv4, pm_server_link_cache_record *out);
/* Only submit records accepted against a current Welcome/live network scope. */
pm_server_link_cache_result pm_server_link_cache_remember(const pm_server_link_cache_record *record);
bool pm_server_link_cache_dirty(void);
/* Conservative (true) on read failure/re-entry. Never modifies pending RAM. */
bool pm_server_link_cache_needs_erase(void);
/* Caller owns idle scheduling and the >=60s successful-commit rate limit.
 * No WiFi disconnect is required. Failure retains pending RAM for later retry.
 */
pm_server_link_cache_result pm_server_link_cache_flush(bool allow_flash, bool allow_erase);
const char *pm_server_link_cache_result_name(pm_server_link_cache_result result);
/* Production Pico SDK adapter; link server_link_cache_flash.c and pico_flash. */
const pm_server_link_cache_io *pm_server_link_cache_flash_io(void);
