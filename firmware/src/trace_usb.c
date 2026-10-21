#include "trace.h"
#include "hardware/sync.h"
#include "tusb.h"
size_t pm_trace_usb_write(const char *bytes,size_t length){
    uint64_t started=pm_stats_clock();
    // TinyUSB CDC FIFO access is bounded and protected from its same-core IRQ.
    // Never call tud_task, sleep, or wait for an endpoint to become writable.
    uint32_t irq=save_and_disable_interrupts();
    size_t written=0;
    if(tud_cdc_connected()){
        uint32_t room=tud_cdc_write_available();
        size_t n=length<room?length:room;
        if(n){written=tud_cdc_write(bytes,(uint32_t)n);tud_cdc_write_flush();}
    }
    restore_interrupts(irq);
    pm_stats_note(PM_STAT_USB_WRITE,started);
    return written;
}
