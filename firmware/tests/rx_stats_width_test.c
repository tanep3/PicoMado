// Serialization limits and full snapshot shape, including maximum integer widths.
#include <assert.h>
#include <string.h>
#include "../src/rx_stats.c"
static unsigned lines,max_width;static char dump[32768];static size_t used;
uint64_t pm_trace_clock(void){return UINT64_MAX;}
uint32_t save_and_disable_interrupts(void){return 0;}
void restore_interrupts(uint32_t s){(void)s;}
int pm_logf(const char *format,...){
    char line[512];va_list args;va_start(args,format);int n=vsnprintf(line,sizeof line,format,args);va_end(args);
    assert(n>0 && n<144 && used+(size_t)n<sizeof dump);if((unsigned)n>max_width)max_width=(unsigned)n;
    memcpy(dump+used,line,(size_t)n);used+=(size_t)n;dump[used]=0;lines++;return n;
}
int main(int argc,char **argv){
    memset(&rx,0xff,sizeof rx);rx.emit_row=0;rx.snapshot_events=4;
    for(unsigned i=0;i<4;i++)rx.snapshot[i].reason=i+1;
    for(unsigned i=0;i<110;i++)if(!pm_rx_emit(UINT32_MAX))break;
    assert(lines==102 && strstr(dump,"rx end epoch=4294967295"));
    if(argc==2){FILE *file=fopen(argv[1],"w");assert(file);fputs(dump,file);assert(!fclose(file));}
    printf("rx width PASS: %u lines, maximum line=%u bytes (<144), fixed RAM=%zu, all 4 records/4 events\n",lines,max_width,pm_rx_ram_bytes());
}
