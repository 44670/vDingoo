#ifndef CCDL_H
#define CCDL_H

#include <stdint.h>

/* ── CCDL Import/Export entry ──────────────────────────────────────────────── */

typedef struct {
    char     name[64];
    uint32_t target_vaddr;   /* stub address in code segment */
} CcdlImport;

typedef struct {
    char     name[64];
    uint32_t vaddr;
} CcdlExport;

/* ── Parsed CCDL binary ───────────────────────────────────────────────────── */

typedef struct {
    CcdlImport *imports;
    int         import_count;
    CcdlExport *exports;
    int         export_count;
    uint32_t    entry_point;    /* CRT _start address */
    uint32_t    load_address;   /* base address for code segment */
    uint32_t    data_size;      /* code+data on disk */
    uint32_t    memory_size;    /* code+data+bss */
    uint32_t    rawd_offset;    /* file offset to RAWD data */
} CcdlBinary;

/* ── RLOC relocation types ────────────────────────────────────────────────── */

#define RELOC_HI16       0   /* lui — high 16 bits */
#define RELOC_LO16       1   /* addiu — sign-extended low 16 */
#define RELOC_LO16U      2   /* ori — zero-extended low 16 */
#define RELOC_J26        3   /* j/jal — 26-bit word-address target */
#define RELOC_DATA32     4   /* raw 32-bit pointer in data */
#define RELOC_LO16_LOAD  5   /* lw/lh/lb offset */
#define RELOC_LO16_STORE 6   /* sw/sh/sb offset */

/* ── Functions ────────────────────────────────────────────────────────────── */

/**
 * Parse a CCDL binary from raw file data.
 * Returns 0 on success, -1 on error.
 * Caller must free ccdl->imports and ccdl->exports.
 */
int ccdl_parse(const uint8_t *data, uint32_t size, CcdlBinary *ccdl);

/**
 * Load RAWD segment into memory at new_base, zero BSS, apply RLOC relocations,
 * and adjust all CCDL addresses.
 *
 * @param ccdl       Parsed CCDL binary (addresses will be updated in-place)
 * @param app_data   Raw .app file data (full file, or NULL to use app_fd)
 * @param reloc_data Raw .reloc.bin file data
 * @param reloc_size Size of reloc data
 * @param new_base   Target load address (e.g. 0x08A00000)
 */
int ccdl_load_relocated(CcdlBinary *ccdl, const uint8_t *app_data,
                        const uint8_t *reloc_data, uint32_t reloc_size,
                        uint32_t new_base);

/**
 * Stream RAWD from an open file descriptor directly into new_base,
 * then apply relocations. For memory-constrained platforms (PSP).
 */
int ccdl_load_relocated_fd(CcdlBinary *ccdl, int app_fd,
                           const uint8_t *reloc_data, uint32_t reloc_size,
                           uint32_t new_base);

/**
 * Patch each import stub with a J instruction to the corresponding HLE function.
 * Must be called AFTER ccdl_load_relocated.
 */
typedef void (*hle_func_t)(void);

typedef struct {
    const char *name;
    hle_func_t  func;
} HleEntry;

int ccdl_patch_imports(CcdlBinary *ccdl, const HleEntry *table, int table_count);

#endif /* CCDL_H */
