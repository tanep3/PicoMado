/* Actual installed lwIP tcp_write, deterministic allocation/link fixtures.
 * No SDK mutation, network, USB or actual radio assumed. */
#define main output_fixture_main
#include "net_nagle_sdk_test.c"
#undef main
#include "lwip/memp.h"
#include "lwip/mem.h"
static unsigned segments;
static bool fail_segment;
void *memp_malloc(memp_t type){assert(type==MEMP_TCP_SEG);if(fail_segment)return NULL;segments++;return calloc(1,sizeof(struct tcp_seg));}
void memp_free(memp_t type,void *p){assert(type==MEMP_TCP_SEG&&segments);segments--;free(p);}
void *mem_malloc(mem_size_t n){return fail_alloc?NULL:malloc(n);}
void mem_free(void *p){free(p);}
void pbuf_cat(struct pbuf *head,struct pbuf *tail){for(;head->next;head=head->next)head->tot_len+=tail->tot_len;head->tot_len+=tail->tot_len;head->next=tail;}
u8_t pbuf_header(struct pbuf *p,s16_t n){p->payload=(uint8_t *)p->payload-n;p->len=(u16_t)(p->len+n);p->tot_len=(u16_t)(p->tot_len+n);return 0;}
u8_t pbuf_add_header(struct pbuf *p,size_t n){return pbuf_header(p,(s16_t)n);}
u16_t pbuf_clen(const struct pbuf *p){u16_t n=0;for(;p;p=p->next)n++;return n;}
void tcp_segs_free(struct tcp_seg *s){while(s){struct tcp_seg *next=s->next;pbuf_free(s->p);memp_free(MEMP_TCP_SEG,s);s=next;}}
static void init_write(struct tcp_pcb *p){setup(p,61209);p->snd_buf=TCP_SND_BUF;p->snd_wnd_max=TCP_WND;p->snd_lbb=100;tcp_nagle_disable(p);}
static void assert_no_copy(struct tcp_pcb *p,uint32_t first,unsigned available,unsigned queued){assert(p->snd_lbb==first&&p->snd_buf==available&&p->snd_queuelen==queued);}
int main(void){
 struct netif n={.linkoutput=actual_link};netif_default=&n;struct tcp_pcb p;const char data[]="credit-absolute";unsigned len=sizeof data;
 init_write(&p);p.snd_buf=1;
 assert(tcp_write(&p,data,len,TCP_WRITE_FLAG_COPY)==ERR_MEM);assert_no_copy(&p,100,1,0);
 init_write(&p);fail_alloc=true;
 assert(tcp_write(&p,data,len,TCP_WRITE_FLAG_COPY)==ERR_MEM);assert_no_copy(&p,100,TCP_SND_BUF,0);assert(!segments);
 fail_alloc=false;fail_segment=true;
 assert(tcp_write(&p,data,len,TCP_WRITE_FLAG_COPY)==ERR_MEM);assert_no_copy(&p,100,TCP_SND_BUF,0);assert(!segments);
 fail_segment=false;
 // Each output becomes unacked, so the next tiny message needs a new segment.
 for(unsigned i=0;i<TCP_SND_QUEUELEN;i++){
  assert(tcp_write(&p,data,len,TCP_WRITE_FLAG_COPY)==ERR_OK);assert(tcp_output(&p)==ERR_OK);
 }
 assert(p.snd_buf>=len&&p.snd_queuelen==TCP_SND_QUEUELEN);
 uint32_t first=p.snd_lbb;unsigned available=p.snd_buf;
 assert(tcp_write(&p,data,len,TCP_WRITE_FLAG_COPY)==ERR_MEM);
 assert_no_copy(&p,first,available,TCP_SND_QUEUELEN);
 // Model release by one peer ACK. Real ACK processing/RF are not simulated.
 struct tcp_seg *acked=p.unacked;assert(acked);p.unacked=acked->next;acked->next=NULL;
 p.snd_buf+=acked->len;p.snd_queuelen--;tcp_segs_free(acked);
 assert(tcp_write(&p,data,len,TCP_WRITE_FLAG_COPY)==ERR_OK);
 assert(p.snd_lbb==first+len&&p.snd_queuelen==TCP_SND_QUEUELEN);
 tcp_segs_free(p.unacked);tcp_segs_free(p.unsent);assert(!segments);
 puts("PASS actual SDK tcp_write: sndbuf, pbuf allocation, segment allocation and tiny-write queue exhaustion reject without COPY; recovery copies once");
}
