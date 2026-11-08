#include "server_link_cache.h"
#include "test_sdk.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>
uint8_t test_flash_bytes[PICO_FLASH_SIZE_BYTES];
static unsigned safety_calls,programs,erases;
static bool in_safe,entry_fail,exit_fail;
int flash_safe_execute(void (*fn)(void *),void *arg,uint32_t timeout) {
    assert(timeout>0&&timeout<=100);++safety_calls;
    if(entry_fail)return -1;
    assert(!in_safe);in_safe=true;fn(arg);in_safe=false;
    return exit_fail?-1:PICO_OK;
}
void flash_range_erase(uint32_t offset,size_t n) {
    assert(in_safe);assert(offset==PICO_FLASH_SIZE_BYTES-16384&&n==4096);
    ++erases;memset(test_flash_bytes+offset,0xff,n);
}
void flash_range_program(uint32_t offset,const uint8_t *page,size_t n) {
    assert(in_safe&&n==256&&offset%256==0);
    assert(offset>=PICO_FLASH_SIZE_BYTES-16384&&offset+256<=PICO_FLASH_SIZE_BYTES-12288);
    ++programs;for(size_t i=0;i<n;i++)test_flash_bytes[offset+i]&=page[i];
}
static void neighbors(void) {
    for(unsigned i=0;i<PICO_FLASH_SIZE_BYTES-16384;i++)assert(test_flash_bytes[i]==0x5a);
    for(unsigned i=PICO_FLASH_SIZE_BYTES-12288;i<PICO_FLASH_SIZE_BYTES;i++)assert(test_flash_bytes[i]==0x5a);
}
int main(void) {
    memset(test_flash_bytes,0x5a,sizeof(test_flash_bytes));
    memset(test_flash_bytes+PICO_FLASH_SIZE_BYTES-16384,0xff,4096);
    const pm_server_link_cache_io *io=pm_server_link_cache_flash_io();uint8_t page[256]={0};
    assert(io->read(NULL,4096,page,1)==PM_SERVER_LINK_CACHE_READ_FAILED);
    assert(io->read(NULL,UINT32_MAX,page,1)==PM_SERVER_LINK_CACHE_READ_FAILED);
    assert(io->program(NULL,4096,page)==PM_SERVER_LINK_CACHE_PROGRAM_FAILED);
    assert(io->program(NULL,1,page)==PM_SERVER_LINK_CACHE_PROGRAM_FAILED);
    assert(io->program(NULL,0,NULL)==PM_SERVER_LINK_CACHE_PROGRAM_FAILED);
    assert(!safety_calls&&!programs&&!erases);
    assert(pm_server_link_cache_load(io)==PM_SERVER_LINK_CACHE_EMPTY);
    pm_server_link_cache_record r={.profile=PM_PROFILE_HOME,.ssid_length=1,.ssid={'a'},
        .server_ipv4=0xc0a80102,.prefix_length=24,.mac={2,1,2,3,4,5}};
    assert(pm_server_link_cache_remember(&r)==PM_SERVER_LINK_CACHE_CHANGED);
    entry_fail=true;assert(pm_server_link_cache_flush(true,true)==PM_SERVER_LINK_CACHE_SAFETY_FAILED);
    assert(!programs&&!erases&&pm_server_link_cache_dirty());neighbors();
    entry_fail=false;exit_fail=true;
    assert(pm_server_link_cache_flush(true,true)==PM_SERVER_LINK_CACHE_SAFETY_FAILED);
    assert(programs==1&&pm_server_link_cache_dirty());neighbors();
    exit_fail=false;assert(pm_server_link_cache_flush(true,true)==PM_SERVER_LINK_CACHE_OK);
    assert(programs==3); /* incomplete first slot is never reprogrammed */
    for(unsigned i=0;i<6;i++) {
        ++r.mac[5];assert(pm_server_link_cache_remember(&r)==PM_SERVER_LINK_CACHE_CHANGED);
        assert(pm_server_link_cache_flush(true,true)==PM_SERVER_LINK_CACHE_OK);
    }
    assert(erases==0);r.mac[5]++;
    assert(pm_server_link_cache_remember(&r)==PM_SERVER_LINK_CACHE_CHANGED);
    assert(pm_server_link_cache_needs_erase());entry_fail=true;
    assert(pm_server_link_cache_flush(true,true)==PM_SERVER_LINK_CACHE_SAFETY_FAILED);
    assert(erases==0&&pm_server_link_cache_dirty());entry_fail=false;
    assert(pm_server_link_cache_flush(true,true)==PM_SERVER_LINK_CACHE_OK);assert(erases==1);neighbors();
    assert(pm_server_link_cache_load(io)==PM_SERVER_LINK_CACHE_OK);
    pm_server_link_cache_record out;
    assert(pm_server_link_cache_get(r.profile,r.ssid,r.ssid_length,r.server_ipv4,&out)&&out.mac[5]==r.mac[5]);
    puts("PASS production SDK adapter offsets/bounds, safety-entry and uncertain-exit faults, readback, neighboring physical flash preservation; hardware stub only");
    return 0;
}
