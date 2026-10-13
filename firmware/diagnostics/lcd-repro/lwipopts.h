#pragma once
// Preserve PicoMado's DHCP/profile behavior. The reproduction uses the YouTube
// background architecture, which requires lwIP's IRQ-safe internal heap.
#include "../../lwipopts.h"
#undef MEM_LIBC_MALLOC
#define MEM_LIBC_MALLOC 0
