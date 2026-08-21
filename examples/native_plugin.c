#include "../../native/neutronium.h"
#include <stdio.h>

/* Build: cc -shared -fPIC examples/native_plugin.c -o libhello.so */
int32_t neut_init(const NeutApi *api) {
    const char message[] = "native module loaded";
    if (!api || api->version != 1 || !api->log) return 1;
    api->log(message, (uint32_t)(sizeof(message) - 1));
    return 0;
}
