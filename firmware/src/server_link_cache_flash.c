/* BT-AUDIO-20261107: Pico SDK IO, confined to FLASH_SIZE - 16KiB sector. */
#include "server_link_cache.h"
#include "hardware/flash.h"
#include "hardware/regs/addressmap.h"
#include "pico/flash.h"
#include "pico/stdlib.h"
#include <string.h>

/* Current firmware launches no core1 and links no multicore/RTOS runtime.
 * SDK flash_safe_execute consequently uses IRQ-only admission. Do not quietly
 * carry that assumption into a multicore build or alter the BT backend policy.
 * Parent also checks the final ELF for a core1 launch entry point.
 */
#if LIB_PICO_MULTICORE || LIB_FREERTOS_KERNEL
#error "Server link cache requires a reviewed multicore flash-safety policy"
#endif
#define CACHE_FLASH_OFFSET (PICO_FLASH_SIZE_BYTES-PM_SERVER_LINK_CACHE_RESERVED_TAIL)
#define CACHE_SAFETY_TIMEOUT_MS 20u
_Static_assert(FLASH_SECTOR_SIZE==PM_SERVER_LINK_CACHE_SECTOR_SIZE,"Unexpected flash sector size");
_Static_assert(FLASH_PAGE_SIZE==PM_SERVER_LINK_CACHE_PAGE_SIZE,"Unexpected flash page size");
_Static_assert(PICO_FLASH_SIZE_BYTES>=PM_SERVER_LINK_CACHE_RESERVED_TAIL,"Flash is too small");
_Static_assert(CACHE_FLASH_OFFSET%FLASH_SECTOR_SIZE==0,"Cache alignment");
_Static_assert(CACHE_FLASH_OFFSET+FLASH_SECTOR_SIZE==PICO_FLASH_SIZE_BYTES-12288u,"BT boundary");

typedef struct {uint32_t offset;const uint8_t *page;bool erase;} operation;
static void __not_in_flash_func(execute_flash_operation)(void *context) {
    operation *op=context;
    if(op->erase)flash_range_erase(CACHE_FLASH_OFFSET,FLASH_SECTOR_SIZE);
    else flash_range_program(CACHE_FLASH_OFFSET+op->offset,op->page,FLASH_PAGE_SIZE);
}
static pm_server_link_cache_result read_cache(void *unused,uint32_t offset,uint8_t *out,size_t n) {
    (void)unused;
    if(!out||offset>FLASH_SECTOR_SIZE||n>FLASH_SECTOR_SIZE-offset)return PM_SERVER_LINK_CACHE_READ_FAILED;
    memcpy(out,(const void *)(XIP_BASE+CACHE_FLASH_OFFSET+offset),n);
    return PM_SERVER_LINK_CACHE_OK;
}
static pm_server_link_cache_result erase_cache(void *unused) {
    (void)unused;operation op={.erase=true};
    int rc=flash_safe_execute(execute_flash_operation,&op,CACHE_SAFETY_TIMEOUT_MS);
    return rc==PICO_OK?PM_SERVER_LINK_CACHE_OK:PM_SERVER_LINK_CACHE_SAFETY_FAILED;
}
static pm_server_link_cache_result program_cache(void *unused,uint32_t offset,const uint8_t *page) {
    (void)unused;
    if(!page||offset%FLASH_PAGE_SIZE||offset>FLASH_SECTOR_SIZE-FLASH_PAGE_SIZE)return PM_SERVER_LINK_CACHE_PROGRAM_FAILED;
    /* The data and context are RAM objects; IRQs are off only inside the SDK
     * admission. Physical erase/program duration is not bounded by its timeout.
     */
    operation op={.offset=offset,.page=page,.erase=false};
    int rc=flash_safe_execute(execute_flash_operation,&op,CACHE_SAFETY_TIMEOUT_MS);
    return rc==PICO_OK?PM_SERVER_LINK_CACHE_OK:PM_SERVER_LINK_CACHE_SAFETY_FAILED;
}
const pm_server_link_cache_io *pm_server_link_cache_flash_io(void) {
    static const pm_server_link_cache_io io={.read=read_cache,.erase=erase_cache,.program=program_cache};
    return &io;
}
