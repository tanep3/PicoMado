/* BT-AUDIO-20261107: production cleanup owner, deterministic sockets only. */
#define main nagle_fixture_main
#include "net_nagle_test.c"
#undef main
static void fresh(void){
    pm_net_close();write_result=output_result=0;fake_sndbuf=65535;
    connecting_profile=&home;pm_net_begin(&home);stage=PM_NET_READY;
    assert(pcb && !terminal_send_failed);aborts=closes=0;
}
static void expect_abort(void){
    assert(stage==PM_NET_FAILED && terminal_send_failed && pcb && !aborts);
    assert(pm_net_checkpoint()==PM_NET_FAILED);
    assert(!pcb && !terminal_send_failed && aborts==1 && !closes);
    pm_net_poll();pm_net_close();assert(aborts==1 && !closes);
}
int main(void){
    // Mandatory byte shortage, no abort while still inside the send adapter.
    fresh();fake_sndbuf=1;assert(!pm_net_send(5,"{}"));expect_abort();
    // Native allocation/segment shortage and other terminal write errors.
    fresh();write_result=ERR_MEM;assert(!pm_net_send(5,"{}"));expect_abort();
    fresh();uint64_t scope=0;write_result=-5;
    assert(pm_net_try_send(25,"{}",&scope)==PM_NET_TX_FAILED);expect_abort();
    // Existing begin/explicit close boundaries must also retire before replace.
    fresh();write_result=ERR_MEM;assert(!pm_net_send(5,"{}"));
    write_result=0;expected_aborts_at_create=1;pm_net_begin(&home);assert(aborts==1 && pcb && !terminal_send_failed);
    stage=PM_NET_READY;assert(pm_net_send(5,"{}"));pm_net_close();assert(closes==1 && aborts==1);
    fresh();write_result=ERR_MEM;assert(!pm_net_send(5,"{}"));pm_net_close();assert(aborts==1 && !pcb);
    // Retryable pressure, successful COPY/output error and optional Close pressure
    // do not change normal graceful cleanup.
    fresh();scope=0;fake_sndbuf=1;assert(pm_net_try_send(25,"{}",&scope)==PM_NET_TX_WAIT);
    fake_sndbuf=65535;write_result=ERR_MEM;assert(pm_net_try_send(25,"{}",&scope)==PM_NET_TX_WAIT);
    assert(pm_net_checkpoint()==PM_NET_READY && !terminal_send_failed && !aborts);
    write_result=0;output_result=-4;assert(pm_net_try_send(25,"{}",&scope)==PM_NET_TX_COPIED);
    assert(!terminal_send_failed);pm_net_close();assert(closes==1 && !aborts);
    fresh();write_result=ERR_MEM;cyw43_arch_lwip_begin();assert(send_close_reply("x",1));cyw43_arch_lwip_end();
    assert(!terminal_send_failed);pm_net_close();assert(closes==1 && !aborts);
    fresh();fake_sndbuf=1;cyw43_arch_lwip_begin();assert(send_close_reply("xx",2));cyw43_arch_lwip_end();
    assert(!terminal_send_failed);pm_net_close();assert(closes==1 && !aborts);
    // Only a live matching send scope can become terminal on expiry.
    fresh();scope=tcp_status.connection_id;pm_net_send_expired(scope-1);
    assert(stage==PM_NET_READY && !terminal_send_failed);pm_net_send_expired(scope);expect_abort();
    fresh();pm_net_send_expired(scope);assert(stage==PM_NET_READY && !terminal_send_failed);
    // Failed handshake runs inside connected callback: return ERR_OK while PCB
    // remains live; main cleanup aborts only after lwIP has unwound that stack.
    fresh();stage=PM_NET_TCP;write_result=ERR_MEM;cyw43_arch_lwip_begin();callback_depth++;
    assert(on_connect(NULL,pcb,ERR_OK)==ERR_OK);assert(pcb && !aborts && terminal_send_failed);
    callback_depth--;cyw43_arch_lwip_end();expect_abort();
    // A real SDK error callback means the SDK has already freed the PCB.
    fresh();write_result=ERR_MEM;assert(!pm_net_send(5,"{}"));
    cyw43_arch_lwip_begin();callback_depth++;on_error(NULL,-14);callback_depth--;cyw43_arch_lwip_end();
    assert(!pcb);pm_net_poll();pm_net_close();assert(!aborts && !closes && !terminal_send_failed);
    // Stale receive callback cannot fail or retire the new current PCB.
    fresh();struct tcp_pcb stale={0};cyw43_arch_lwip_begin();callback_depth++;
    assert(recv_cb(NULL,&stale,NULL,-14)==ERR_OK);callback_depth--;cyw43_arch_lwip_end();
    assert(stage==PM_NET_READY && !terminal_send_failed);pm_net_close();assert(closes==1 && !aborts);
    // Normal peer EOF still enters draining, without a terminal-send latch.
    fresh();cyw43_arch_lwip_begin();callback_depth++;
    assert(on_receive(NULL,pcb,NULL,ERR_OK)==ERR_OK);callback_depth--;cyw43_arch_lwip_end();
    assert(stage==PM_NET_DRAINING && !terminal_send_failed);pm_net_close();assert(closes==1 && !aborts);
    puts("PASS production terminal cleanup: send/timeout, WAIT/COPY, callback unwind, detach, SDK free, scopes and graceful EOF");
    return 0;
}
