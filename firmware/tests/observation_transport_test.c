#include <assert.h>
#include <stdio.h>
#include "observation_transport.h"
int main(void){
    pm_ob_tx_map tx={0};
    pm_ob_tx_publish(&tx,100,140,10,1);
    assert(pm_ob_tx_find(&tx,100,120).copy==10);
    assert(pm_ob_tx_find(&tx,120,140).notify==1);
    assert(pm_ob_tx_find(&tx,100,140).copy==10); /* retransmission */
    assert(!pm_ob_tx_find(&tx,90,120).copy&&!pm_ob_tx_find(&tx,120,150).copy);
    pm_ob_tx_publish(&tx,140,160,11,2);
    assert(!pm_ob_tx_find(&tx,120,150).copy); /* coalesced two causes */
    for(unsigned i=0;i<8;i++)pm_ob_tx_publish(&tx,160+i*10,170+i*10,20+i,3);
    assert(!pm_ob_tx_find(&tx,100,120).copy); /* bounded eviction */
    assert(pm_ob_tx_find(&tx,230,240).copy==27);
    pm_ob_tx_publish(&tx,240,250,0,0); /* untraced bytes still track sequence */
    pm_ob_tx_publish(&tx,250,260,30,4);
    assert(pm_ob_tx_find(&tx,250,260).copy==30);
    pm_ob_tx_publish(&tx,UINT32_MAX-5,4,31,5);
    assert(tx.wrapped&&!pm_ob_tx_find(&tx,250,260).copy);
    memset(&tx,0,sizeof tx);pm_ob_tx_publish(&tx,1,5,40,6);
    assert(pm_ob_tx_find(&tx,1,5).copy==40); /* connection reset */
    pm_ob_rx_map rx={0};pm_ob_rx_publish(&rx,100,140,10);pm_ob_rx_publish(&rx,140,160,11);
    assert(pm_ob_rx_find(&rx,102,120)==10);
    assert(!pm_ob_rx_find(&rx,120,150)); /* spans callbacks: no guessed parent */
    pm_ob_app_map app={0};uint32_t first,end;
    pm_ob_app_append(&app,100,10);pm_ob_app_append(&app,110,10);
    assert(app.count==1&&pm_ob_app_take(&app,15,&first,&end)&&first==100&&end==115);
    pm_ob_app_append(&app,122,10); /* WS header gap */
    assert(pm_ob_app_take(&app,15,&first,&end)&&first==115&&end==132&&!app.count);
    for(unsigned i=0;i<9;i++)pm_ob_app_append(&app,200+2*i,1);
    assert(app.unknown&&!pm_ob_app_take(&app,1,&first,&end));
    puts("PASS bounded TCP/RX provenance: split, retransmission, coalescing unknown, eviction, wrap/reset and WS span gaps");
}
