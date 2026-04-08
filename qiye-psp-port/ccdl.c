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

/* ── Patch SPECIAL2 → Allegrex SPECIAL ────────────────────────────────────── */
/*
 * Dingoo MIPS32R1 uses SPECIAL2 (opcode 0x1C) for mul/madd/msub.
 * PSP Allegrex uses SPECIAL (opcode 0x00) with different func codes.
 *
 * SPECIAL2 madd (func=0) → SPECIAL madd (func=0x1C)   [same rd=0 encoding]
 * SPECIAL2 msub (func=4) → SPECIAL msub (func=0x2E)
 * SPECIAL2 mul rd,rs,rt (func=2) → mult rs,rt (func=0x18) at [i],
 *                                   mflo rd   (func=0x12) at [i+1],
 *                                   original [i+1] shifted to [i+2]... NO.
 *
 * For mul rd,rs,rt we can't expand in-place. Instead:
 *   Replace mul with mult rs,rt (writes HI:LO), then the NEXT instruction
 *   must be replaced with mflo rd. This means we need to move the next
 *   instruction. But we can't grow the code.
 *
 * Trick: mul rd,rs,rt actually writes rd AND HI:LO on MIPS32R1.
 * On Allegrex, mult rs,rt writes HI:LO. So we replace:
 *   mul rd,rs,rt  →  mult rs,rt   (overwrite in-place)
 * and insert mflo rd after it. But to "insert" we'd overwrite the next insn.
 *
 * Better: use a NOP-slide approach. The compiler typically emits:
 *   mul rd,rs,rt   (single insn does multiply and stores to rd)
 * We need TWO instructions. So we scan forward for a NOP or use the delay
 * slot trick. Actually — let's just overwrite the mul AND the next insn:
 *   [i]   = mult rs,rt
 *   [i+1] = mflo rd
 * and relocate the original [i+1] forward... NO, can't do that.
 *
 * FINAL APPROACH: generate a trampoline table in unused memory.
 * Each mul rd,rs,rt is replaced with J trampoline; NOP.
 * The trampoline does: mult rs,rt; mflo rd; J (return_addr); NOP.
 * Return addr = original_mul_addr + 4 (the next instruction).
 *
 * For madd/msub: simple 1:1 opcode+func remapping, no expansion needed.
 */

static uint32_t *g_trampoline_ptr;

static uint32_t make_j_addr(uint32_t addr) {
    return 0x08000000 | ((addr >> 2) & 0x03FFFFFF);
}

static void patch_special2(uint32_t code_base, uint32_t code_size,
                           uint32_t trampoline_base) {
    uint32_t *code = (uint32_t *)code_base;
    uint32_t count = code_size / 4;
    g_trampoline_ptr = (uint32_t *)trampoline_base;
    uint32_t n_madd = 0, n_mul = 0, n_msub = 0;

    for (uint32_t i = 0; i < count; i++) {
        uint32_t insn = code[i];
        uint32_t op = (insn >> 26) & 0x3F;
        if (op != 0x1C) continue; /* not SPECIAL2 */

        uint32_t rs = (insn >> 21) & 0x1F;
        uint32_t rt = (insn >> 16) & 0x1F;
        uint32_t rd = (insn >> 11) & 0x1F;
        uint32_t func = insn & 0x3F;

        if (func == 0x00) {
            /* madd rs,rt: SPECIAL2 func=0 → SPECIAL func=0x1C */
            code[i] = (rs << 21) | (rt << 16) | 0x1C; /* op=0, func=0x1C */
            n_madd++;
        } else if (func == 0x04) {
            /* msub rs,rt: SPECIAL2 func=4 → SPECIAL func=0x2E */
            code[i] = (rs << 21) | (rt << 16) | 0x2E; /* op=0, func=0x2E */
            n_msub++;
        } else if (func == 0x02) {
            /* mul rd,rs,rt → trampoline: mult rs,rt; mflo rd; j ret; nop */
            uint32_t ret_addr = code_base + (i + 1) * 4;
            uint32_t tramp_addr = (uint32_t)g_trampoline_ptr;

            /* Write trampoline */
            g_trampoline_ptr[0] = (rs << 21) | (rt << 16) | 0x18; /* mult rs,rt */
            g_trampoline_ptr[1] = (rd << 11) | 0x12;              /* mflo rd */
            g_trampoline_ptr[2] = make_j_addr(ret_addr);          /* j ret */
            g_trampoline_ptr[3] = 0x00000000;                     /* nop */
            g_trampoline_ptr += 4;

            /* Replace mul with: j trampoline; nop (delay slot) */
            /* But the delay slot [i+1] is a real instruction — we can't NOP it!
             * Instead: j trampoline puts the DELAY SLOT instruction [i+1] in
             * the trampoline before the j-return. Actually j executes delay slot.
             * So: code[i] = j tramp; code[i+1] stays (it's the delay slot, runs
             * before the jump takes effect, but we DON'T want it to run before
             * the multiply).
             *
             * Hmm, j has a delay slot — code[i+1] executes BEFORE the jump.
             * That's fine as long as code[i+1] doesn't depend on the mul result
             * (which it could!). This is tricky.
             *
             * Simplest safe approach:
             *   code[i]   = j trampoline
             *   code[i+1] = nop  (sacrifice the delay slot)
             *   trampoline: mult rs,rt; mflo rd; original_code[i+1]; j ret+4; nop
             *   where ret+4 = code_base + (i+2)*4
             */
            uint32_t orig_next = code[i + 1]; /* save before overwriting */
            ret_addr = code_base + (i + 2) * 4; /* skip both mul and nop */

            /* Rewrite trampoline with saved next insn */
            g_trampoline_ptr -= 4; /* back up */
            g_trampoline_ptr[0] = (rs << 21) | (rt << 16) | 0x18; /* mult rs,rt */
            g_trampoline_ptr[1] = (rd << 11) | 0x12;              /* mflo rd */
            g_trampoline_ptr[2] = make_j_addr(ret_addr);          /* j ret */
            g_trampoline_ptr[3] = orig_next;                      /* delay slot: original next insn */
            g_trampoline_ptr += 4;

            code[i]     = make_j_addr(tramp_addr);
            code[i + 1] = 0x00000000; /* nop */
            i++; /* skip the nop we just wrote */
            n_mul++;
        }
    }

    printf("[PATCH] SPECIAL2→Allegrex: madd=%lu mul=%lu msub=%lu (trampoline: %lu bytes)\n",
           (unsigned long)n_madd, (unsigned long)n_mul, (unsigned long)n_msub,
           (unsigned long)((uint32_t)g_trampoline_ptr - trampoline_base));
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

    /* Patch SPECIAL2 instructions (mul/madd/msub) for Allegrex compatibility.
     * Trampoline goes right after BSS. */
    uint32_t tramp_base = new_base + ccdl->memory_size;
    patch_special2(new_base, ccdl->data_size, tramp_base);

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
