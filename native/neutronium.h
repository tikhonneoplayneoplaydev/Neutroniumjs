#ifndef NEUTRONIUM_H
#define NEUTRONIUM_H
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
/* Stable ABI: ownership never crosses the boundary. Return 0 on success. */
typedef struct NeutApi { uint32_t version; void (*log)(const char *message, uint32_t length); } NeutApi;
typedef int32_t (*neut_init_fn)(const NeutApi *api);
#ifdef __cplusplus
}
#endif
#endif
