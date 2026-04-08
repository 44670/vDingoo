#include "ccdl.h"
#include <string.h>
#include <stdlib.h>
#include <stdio.h>

#ifdef _PSP
#include <psputility.h>
#include <pspkernel.h>
#endif

/* ── Helpers ──────────────────────────────────────────────────────────────── */

static uint32_t rd32(const uint8_t *p) {
    return p[0] | (p[1] << 8) | (p[2] << 16) | (p[3] << 24);
}

/* ── CCDL parsing ─────────────────────────────────────────────────────────── */

static int parse_section(const uint8_t *data, int off, const char *magic,
                         uint32_t *data_off, uint32_t *data_sz) {
    if (memcmp(data + off, magic, 4) != 0) {
        printf("[CCDL] bad section magic at 0x%x\n", off);
        return -1;
    }
    *data_off = rd32(data + off + 8);
    *data_sz  = rd32(data + off + 12);
    return 0;
}

static int parse_table(const uint8_t *data, uint32_t table_off,
                       CcdlImport **out_imports, CcdlExport **out_exports,
                       int *out_count, int is_import) {
    const uint8_t *base = data + table_off;
    int count = (int)rd32(base);
    const uint8_t *entries = base + 16;
    const uint8_t *names  = entries + count * 16;

    *out_count = count;

    if (is_import) {
        *out_imports = (CcdlImport *)calloc(count, sizeof(CcdlImport));
        for (int i = 0; i < count; i++) {
            const uint8_t *e = entries + i * 16;
            uint32_t name_off = rd32(e);
            uint32_t vaddr    = rd32(e + 12);
            const char *name  = (const char *)(names + name_off);
            strncpy((*out_imports)[i].name, name, 63);
            (*out_imports)[i].target_vaddr = vaddr;
        }
    } else {
        *out_exports = (CcdlExport *)calloc(count, sizeof(CcdlExport));
        for (int i = 0; i < count; i++) {
            const uint8_t *e = entries + i * 16;
            uint32_t name_off = rd32(e);
            uint32_t vaddr    = rd32(e + 12);
            const char *name  = (const char *)(names + name_off);
            strncpy((*out_exports)[i].name, name, 63);
            (*out_exports)[i].vaddr = vaddr;
        }
    }
    return 0;
}

int ccdl_parse(const uint8_t *data, uint32_t size, CcdlBinary *ccdl) {
    (void)size;
    memset(ccdl, 0, sizeof(*ccdl));

    if (memcmp(data, "CCDL", 4) != 0) {
        printf("[CCDL] bad magic\n");
        return -1;
    }

    uint32_t impt_off, impt_sz, expt_off, expt_sz, rawd_off, rawd_sz;
    if (parse_section(data, 0x20, "IMPT", &impt_off, &impt_sz) < 0) return -1;
    if (parse_section(data, 0x40, "EXPT", &expt_off, &expt_sz) < 0) return -1;
    if (parse_section(data, 0x60, "RAWD", &rawd_off, &rawd_sz) < 0) return -1;

    ccdl->entry_point   = rd32(data + 0x74);
    ccdl->load_address  = rd32(data + 0x78);
    ccdl->memory_size   = rd32(data + 0x7C);
    ccdl->data_size     = rawd_sz;
    ccdl->rawd_offset   = rawd_off;

    parse_table(data, impt_off, &ccdl->imports, NULL, &ccdl->import_count, 1);
    parse_table(data, expt_off, NULL, &ccdl->exports, &ccdl->export_count, 0);

    printf("[CCDL] parsed: %d imports, %d exports\n", ccdl->import_count, ccdl->export_count);
    printf("[CCDL] load=0x%08lx entry=0x%08lx data=%lu bss=%lu\n",
           (unsigned long)ccdl->load_address, (unsigned long)ccdl->entry_point,
           (unsigned long)ccdl->data_size,
           (unsigned long)(ccdl->memory_size - ccdl->data_size));

    return 0;
}

/* ── Relocation ───────────────────────────────────────────────────────────── */

static int apply_relocs(const uint8_t *reloc_data, uint32_t reloc_size,
                        uint32_t new_base, uint32_t delta) {
    (void)reloc_size;

    if (memcmp(reloc_data, "RLOC", 4) != 0) {
        printf("[RELOC] bad magic\n");
        return -1;
    }

    uint32_t count = rd32(reloc_data + 8);
    uint16_t delta_hi = (uint16_t)(delta >> 16);
    uint32_t delta_j  = (delta >> 2) & 0x03FFFFFF;

    uint32_t counts[7] = {0};

    for (uint32_t i = 0; i < count; i++) {
        const uint8_t *e = reloc_data + 16 + i * 8;
        uint32_t offset = rd32(e);
        uint16_t rtype  = e[4] | (e[5] << 8);

        uint32_t addr = new_base + offset;
        uint32_t *p = (uint32_t *)addr;

        switch (rtype) {
        case RELOC_HI16: {
            uint32_t insn = *p;
            uint16_t old_imm = (uint16_t)(insn & 0xFFFF);
            uint16_t new_imm = old_imm + delta_hi;
            *p = (insn & 0xFFFF0000) | new_imm;
            counts[0]++;
            break;
        }
        case RELOC_LO16:
        case RELOC_LO16U:
        case RELOC_LO16_LOAD:
        case RELOC_LO16_STORE:
            /* No change needed: delta & 0xFFFF == 0 */
            counts[rtype]++;
            break;
        case RELOC_J26: {
            uint32_t insn = *p;
            uint32_t old_t = insn & 0x03FFFFFF;
            uint32_t new_t = (old_t + delta_j) & 0x03FFFFFF;
            *p = (insn & 0xFC000000) | new_t;
            counts[3]++;
            break;
        }
        case RELOC_DATA32: {
            uint32_t val = *p;
            *p = val + delta;
            counts[4]++;
            break;
        }
        default:
            printf("[RELOC] unknown type %d at offset 0x%06lx\n", rtype, (unsigned long)offset);
            return -1;
        }
    }

    printf("[RELOC] applied %lu: HI16=%lu LO16=%lu J26=%lu DATA32=%lu\n",
           (unsigned long)count, (unsigned long)counts[0],
           (unsigned long)(counts[1] + counts[2] + counts[5] + counts[6]),
           (unsigned long)counts[3], (unsigned long)counts[4]);
    return 0;
}

int ccdl_load_relocated(CcdlBinary *ccdl, const uint8_t *app_data,
                        const uint8_t *reloc_data, uint32_t reloc_size,
                        uint32_t new_base) {
    uint32_t old_base = ccdl->load_address;
    uint32_t delta = new_base - old_base;

    printf("[RELOC] 0x%08lx -> 0x%08lx (delta=0x%08lx)\n",
           (unsigned long)old_base, (unsigned long)new_base, (unsigned long)delta);

    /* Copy code+data to new address */
    memcpy((void *)new_base, app_data + ccdl->rawd_offset, ccdl->data_size);

    /* Zero BSS */
    uint32_t bss_size = ccdl->memory_size - ccdl->data_size;
    if (bss_size > 0) {
        memset((void *)(new_base + ccdl->data_size), 0, bss_size);
    }

    /* Apply relocations */
    if (apply_relocs(reloc_data, reloc_size, new_base, delta) < 0)
        return -1;

    /* Adjust CCDL addresses */
    ccdl->load_address = new_base;
    ccdl->entry_point += delta;
    for (int i = 0; i < ccdl->import_count; i++)
        ccdl->imports[i].target_vaddr += delta;
    for (int i = 0; i < ccdl->export_count; i++)
        ccdl->exports[i].vaddr += delta;

    printf("[RELOC] done: entry=0x%08lx load=0x%08lx\n",
           (unsigned long)ccdl->entry_point, (unsigned long)ccdl->load_address);
    return 0;
}

/* ── Streaming load (PSP — read RAWD from fd, no full-file malloc) ────────── */

int ccdl_load_relocated_fd(CcdlBinary *ccdl, int app_fd,
                           const uint8_t *reloc_data, uint32_t reloc_size,
                           uint32_t new_base) {
    uint32_t old_base = ccdl->load_address;
    uint32_t delta = new_base - old_base;

    printf("[RELOC] 0x%08lx -> 0x%08lx (delta=0x%08lx)\n",
           (unsigned long)old_base, (unsigned long)new_base, (unsigned long)delta);

    /* Seek to RAWD data in file and read directly into target address */
#ifdef _PSP
    sceIoLseek(app_fd, ccdl->rawd_offset, PSP_SEEK_SET);
    int rd = sceIoRead(app_fd, (void *)new_base, ccdl->data_size);
#else
    lseek(app_fd, ccdl->rawd_offset, SEEK_SET);
    int rd = read(app_fd, (void *)new_base, ccdl->data_size);
#endif
    if (rd < 0 || (uint32_t)rd != ccdl->data_size) {
        printf("[RELOC] failed to read RAWD: got %d, expected %lu\n",
               rd, (unsigned long)ccdl->data_size);
        return -1;
    }
    printf("[RELOC] read %lu bytes RAWD to 0x%08lx\n",
           (unsigned long)ccdl->data_size, (unsigned long)new_base);

    /* Zero BSS */
    uint32_t bss_size = ccdl->memory_size - ccdl->data_size;
    if (bss_size > 0) {
        memset((void *)(new_base + ccdl->data_size), 0, bss_size);
    }

    /* Apply relocations */
    if (apply_relocs(reloc_data, reloc_size, new_base, delta) < 0)
        return -1;

    /* Adjust CCDL addresses */
    ccdl->load_address = new_base;
    ccdl->entry_point += delta;
    for (int i = 0; i < ccdl->import_count; i++)
        ccdl->imports[i].target_vaddr += delta;
    for (int i = 0; i < ccdl->export_count; i++)
        ccdl->exports[i].vaddr += delta;

    printf("[RELOC] done: entry=0x%08lx load=0x%08lx\n",
           (unsigned long)ccdl->entry_point, (unsigned long)ccdl->load_address);
    return 0;
}

/* ── Import stub patching ─────────────────────────────────────────────────── */

static uint32_t make_j(void *target) {
    uint32_t addr = (uint32_t)target;
    return 0x08000000 | ((addr >> 2) & 0x03FFFFFF);
}

int ccdl_patch_imports(CcdlBinary *ccdl, const HleEntry *table, int table_count) {
    int patched = 0, unresolved = 0;

    for (int i = 0; i < ccdl->import_count; i++) {
        const char *name = ccdl->imports[i].name;
        uint32_t stub_addr = ccdl->imports[i].target_vaddr;
        hle_func_t func = NULL;

        for (int j = 0; j < table_count; j++) {
            if (strcmp(name, table[j].name) == 0) {
                func = table[j].func;
                break;
            }
        }

        if (func) {
            /* Overwrite stub with: J func; NOP */
            uint32_t *code = (uint32_t *)stub_addr;
            code[0] = make_j(func);
            code[1] = 0x00000000; /* NOP delay slot */
            patched++;
        } else {
            printf("[HLE] WARN: unresolved import '%s'\n", name);
            unresolved++;
        }
    }

    /* Flush caches after code modification */
#ifdef _PSP
    sceKernelDcacheWritebackAll();
    sceKernelIcacheInvalidateAll();
#endif

    printf("[HLE] patched %d/%d import stubs (%d unresolved)\n",
           patched, ccdl->import_count, unresolved);
    return 0;
}
