# HLE API Prototypes

Dingoo A320 SDK functions imported by CCDL apps. Prototypes confirmed from
caller disassembly (MIPS o32: args in `$a0-$a3`, return in `$v0`).

CCDL stub types in unpatched binary:
- `nop; jr $ra; nop` — returns 0 (safe no-op)
- `lw $zero, 1; nop` — trap (must be patched by loader)

## C stdlib

```c
void* malloc(u32 size);                              // 0x80a001f8
void* realloc(void* ptr, u32 size);                  // 0x80a00200
void  free(void* ptr);                               // 0x80a00208
u32   strlen(const char* s);                         // 0x80a00250
int   strncasecmp(const char* a, const char* b, u32 n); // 0x80a001f0
int   printf(const char* fmt, ...);                  // 0x80a001d8
int   sprintf(char* buf, const char* fmt, ...);      // 0x80a001e0
int   fprintf(void* stream, const char* fmt, ...);   // 0x80a001e8
void  abort(void);                                   // 0x80a001d0
```

## Filesystem (fsys_*)

File handle is an opaque `u32`. Mirrors standard C stdio signatures.

```c
u32  fsys_fopen(const char* path, const char* mode);  // 0x80a002d0
u32  fsys_fopenW(const u16* path, const char* mode);  // 0x80a003f0  (unicode path)
u32  fsys_fread(void* buf, u32 size, u32 count, u32 fp);  // 0x80a002d8
u32  fsys_fwrite(const void* buf, u32 size, u32 count, u32 fp); // 0x80a00318
int  fsys_fseek(u32 fp, i32 offset, int whence);      // 0x80a002e8  (0=SET,2=END)
u32  fsys_ftell(u32 fp);                              // 0x80a002f0
int  fsys_fclose(u32 fp);                             // 0x80a002e0
int  fsys_ferror(u32 fp);                             // 0x80a00308
int  fsys_feof(u32 fp);                               // 0x80a00310
int  fsys_remove(const char* path);                   // 0x80a002f8
int  fsys_rename(const char* old, const char* new);   // 0x80a00300
u32  fsys_findfirst(const char* pattern, void* info);  // 0x80a00320
int  fsys_findnext(u32 handle);                       // 0x80a00328
int  fsys_findclose(u32 handle);                      // 0x80a00330
void fsys_flush_cache(void);                          // 0x80a00338
void fsys_RefreshCache(void);                         // 0x80a00248
```

## C file I/O (host-side, used by fprintf)

```c
u32  fread(void* buf, u32 size, u32 count, u32 fp);   // 0x80a00210
u32  fwrite(const void* buf, u32 size, u32 count, u32 fp); // 0x80a00218
int  fseek(u32 fp, i32 offset, int whence);            // 0x80a00220
```

## LCD / Display

320×240, 16-bit RGB565 framebuffer.

```c
int   LcdGetDisMode(void);                            // 0x80a00228
void  _lcd_set_frame(void);                           // 0x80a00258  (no args — flips internal buf)
void* _lcd_get_frame(void);                           // 0x80a00260  (returns framebuf ptr)
void* lcd_get_cframe(void);                           // 0x80a00268  (returns current display buf)
void  ap_lcd_set_frame(void* buf);                    // 0x80a00270
void  lcd_flip(void);                                 // 0x80a00278
```

## Input

```c
u32  _kbd_get_key(u32* key_out, u32* status_out);     // 0x80a002c8
u32  _kbd_get_status(void);                           // 0x80a002b8
u32  get_game_vol(void);                              // 0x80a002c0
```

## Audio (waveout / pcm)

Params struct at offset passed to waveout_open:
```c
struct waveout_params {   // filled before waveout_open call
    u32 sample_rate;      // +0x00  (0x3e80 = 16000 Hz)
    u16 bits_per_sample;  // +0x04  (0x10 = 16-bit)
    // +0x06 padding
    u8  channels;         // +0x08  (1 = mono)
    u8  volume;           // +0x09  (0x64 = 100)
};
```

```c
u32  waveout_open(waveout_params* params);             // 0x80a00358  → handle
void waveout_close(u32 handle);                        // 0x80a00360
void waveout_close_at_once(u32 handle);                // 0x80a00368
void waveout_set_volume(u32 handle, u32 vol);          // 0x80a00370
int  waveout_can_write(u32 handle);                    // 0x80a00380
void waveout_write(u32 handle, void* buf, u32 len);    // 0x80a00388  (len=0x320)
int  pcm_can_write(u32 handle);                        // 0x80a00390
int  pcm_ioctl(u32 handle, u32 cmd);                   // 0x80a00398
void HP_Mute_sw(u32 mute);                             // 0x80a00378
```

## RTOS (uC/OS-II)

Standard uC/OS-II API. `OS_EVENT*` is opaque pointer.

```c
OS_EVENT* OSSemCreate(u32 cnt);                        // 0x80a003c0
u8   OSSemDel(OS_EVENT* sem, u8 opt, u8* err);        // 0x80a003d0  (opt: 1=DEL_ALWAYS)
void OSSemPend(OS_EVENT* sem, u32 timeout, u8* err);  // 0x80a003b0  (timeout: -1=forever)
u8   OSSemPost(OS_EVENT* sem);                         // 0x80a003b8
u8   OSTaskCreate(void (*task)(void*), void* pdata, void* ptos, u8 prio); // 0x80a003c8
u8   OSTaskDel(u8 prio);                               // 0x80a003d8  (0xff=OS_PRIO_SELF)
u32  OSTimeGet(void);                                  // 0x80a003a0  (returns tick count)
void OSTimeDly(u32 ticks);                             // 0x80a003a8  (caller: µs/10000)
u32  OSCPUSaveSR(void);                                // 0x80a00298  (disable interrupts)
void OSCPURestoreSR(u32 sr);                           // 0x80a002a0
```

## System

```c
void vxGoHome(void);                                   // 0x80a00230  (exit to OS menu)
void StartSwTimer(void* callback, u32 interval);       // 0x80a00238
void free_irq(u32 irq);                               // 0x80a00240
u32  GetTickCount(void);                               // 0x80a003e0  (ms since boot)
void _sys_judge_event(void);                           // 0x80a003e8  (poll system events)
void TaskMediaFunStop(void);                           // 0x80a00290
u32  USB_Connect(void);                                // 0x80a00340
u32  udc_attached(void);                               // 0x80a00348
void USB_No_Connect(void);                             // 0x80a00350
```

## Serial (debug)

```c
int  serial_getc(void);                                // 0x80a002a8
void serial_putc(int c);                               // 0x80a002b0
```

## Cache (no-op in HLE)

```c
void __icache_invalidate_all(void);                    // 0x80a00280
void __dcache_writeback_all(void);                     // 0x80a00288
```

## Locale / Unicode

```c
int  __to_unicode_le(u16* out, const char* in, u32 len); // 0x80a003f8
int  __to_locale_ansi(char* out, const u16* in, u32 len); // 0x80a00400
int  get_current_language(void);                       // 0x80a00408  (0=CN, 1=EN?)
```

## HLE Priority

For a minimal boot, implement in this order:

1. **Must have**: malloc/free/realloc, printf/sprintf, strlen
2. **Must have**: fsys_fopen/fread/fclose/fseek/ftell (game loads all assets via fsys)
3. **Must have**: _lcd_get_frame/lcd_flip (need framebuffer output to see anything)
4. **Must have**: _kbd_get_key (input)
5. **Must have**: OSSemCreate/Pend/Post, OSTaskCreate, OSTimeDly (audio thread)
6. **Should have**: waveout_open/write/close (audio)
7. **Should have**: GetTickCount/OSTimeGet (timing)
8. **Stub**: cache ops, serial, USB, _sys_judge_event, vxGoHome
