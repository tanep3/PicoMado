#pragma once
#include <stdint.h>
#include <stdlib.h>
#include <stdio.h>
#define LWIP_PLATFORM_DIAG(x) do { printf x; } while(0)
#define LWIP_PLATFORM_ASSERT(x) do { fprintf(stderr,"lwIP assertion: %s\n",x);abort(); } while(0)
