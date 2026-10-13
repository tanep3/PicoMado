#pragma once
#include <stdbool.h>
// RFC 6455: expected 28-character Sec-WebSocket-Accept value.
bool pm_ws_accept(const char *client_key, char out[29]);
