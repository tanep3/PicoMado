#include <assert.h>
#include <string.h>
#include "ws_accept.h"
int main(void){
    char value[29];
    assert(pm_ws_accept("dGhlIHNhbXBsZSBub25jZQ==",value));
    assert(!strcmp(value,"s3pPLMBiTxaQ9kYGzzhZRbK+xOo="));
    assert(!pm_ws_accept("short",value));
    return 0;
}
