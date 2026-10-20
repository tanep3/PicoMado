#include <stdio.h>
#include <stdint.h>
#include <time.h>
#include <stdlib.h>
#include <string.h>
#include "ack_probe.h"
static volatile uint64_t sink;
static uint64_t ns(void){struct timespec t;clock_gettime(CLOCK_MONOTONIC_RAW,&t);return (uint64_t)t.tv_sec*1000000000+t.tv_nsec;}
uint64_t time_us_64(void){return ns()/1000;}
bool pm_trace_try_line(const char *p,size_t n){(void)p;(void)n;return false;}
static int compare(const void *a,const void *b){uint64_t x=*(const uint64_t *)a,y=*(const uint64_t *)b;return (x>y)-(x<y);}
int main(void){
 char json[301];memset(json,'x',sizeof json-1);json[300]=0;uint64_t samples[2000];
 for(unsigned i=0;i<2000;i++){
  pm_ack_probe_init_boot(1);pm_ack_probe_command("AP2 ARM 1 1");uint64_t start=ns();
  pm_ack_probe_button(1,1,1,1,false);pm_ack_probe_context(1,2);
  pm_ack_probe_note(PM_AP_ACK_BUILD,9,1,2,0,0,0,0);
  pm_ack_probe_before_lock(9,json);pm_ack_probe_locked(9);pm_ack_probe_encoded(9,json);
  pm_ack_probe_net(PM_AP_WRITE_BEGIN,1,1,1,100,100);pm_ack_probe_net(PM_AP_WRITE_END,3,0,100,1,100);pm_ack_probe_net(PM_AP_OUTPUT_END,5,0,100,1,100);pm_ack_probe_send_return(9,true);
  pm_ack_probe_note(PM_AP_PENDING_TAKE,9,1,2,0,0,0,0);sink+=i;
  samples[i]=ns()-start;
 }
 qsort(samples,2000,sizeof samples[0],compare);
 printf("{\"probe\":%u,\"host_transaction_ns_median\":%llu,\"host_transaction_ns_p95\":%llu,\"host_transaction_ns_max\":%llu,\"json_bytes\":300,\"trials\":2000}\n",PM_ACK_PATH_PROBE,(unsigned long long)samples[1000],(unsigned long long)samples[1900],(unsigned long long)samples[1999]);
}
