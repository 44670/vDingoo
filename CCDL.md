# CCDL Binary Format

The CCDL (ChinaChip Dynamic Loader) format is the native executable format for
**Dingoo** handheld game consoles (A320, A330, etc.). These devices use an
**Actions Semiconductor** MIPS SoC running the **uC/OS-II** real-time operating
system. Files use the `.app` extension.

CCDL is a simple relocatable module format. The OS loader reads the headers,
loads the code segment into memory at the base address, patches import
relocations to resolve OS/SDK function addresses, runs the CRT entry point
(which performs BSS zeroing and framebuffer allocation), then creates an
RTOS task for the exported `AppMain` function.

---

## File Layout

```
+------------------+----------+------------------------------------------+
| Region           | Offset   | Size                                     |
+------------------+----------+------------------------------------------+
| CCDL Header      | 0x000    | 0x20 (32 bytes)                          |
| IMPT Header      | 0x020    | 0x20 (32 bytes)                          |
| EXPT Header      | 0x040    | 0x20 (32 bytes)                          |
| RAWD Header      | 0x060    | 0x20 (32 bytes) + 0x20 padding           |
| Import Table     | varies   | sub-header + entries + name strings      |
| Export Table     | varies   | sub-header + entries + name strings      |
| Code + Data      | varies   | raw MIPS code loaded at load address     |
| Padding          | varies   | 0xFF fill to next aligned boundary       |
| Resource Pack    | varies   | optional embedded resource filesystem     |
+------------------+----------+------------------------------------------+
```

---

## CCDL Header (0x00 - 0x1F, 32 bytes)

```
Offset  Size  Field
------  ----  -----
0x00    4     Magic: "CCDL" (43 43 44 4C)
0x04    2     Reserved (0x0000)
0x06    2     Format version (0x0001 = v1.0)
0x08    2     Unknown flags
0x0A    2     Unknown flags
0x0C    4     Unknown flags
0x10    7     Build timestamp (BCD-encoded: YY MM DD HH MM SS ??)
              Example: 20 09 07 15 11 04 43 → 2020-09-07 15:11:04
0x17    9     Reserved (zero-filled)
```

---

## Section Headers (IMPT / EXPT / RAWD)

Each section header is 32 bytes with a common layout:

```
Offset  Size  Field
------  ----  -----
0x00    4     Magic: "IMPT", "EXPT", or "RAWD"
0x04    4     Section type ID
              IMPT = 0x08, EXPT = 0x09, RAWD = 0x01
0x08    4     Data offset (absolute file offset to section data)
0x0C    4     Data size (bytes)
0x10    16    Section-specific fields (see below)
```

### IMPT Section-Specific Fields

All zeros (reserved).

### EXPT Section-Specific Fields

All zeros (reserved).

### RAWD Section-Specific Fields (0x60 - 0x7F)

```
Offset  Size  Field
------  ----  -----
0x10    4     Reserved (0)
0x14    4     Entry point virtual address (CRT init code entry)
0x18    4     Load address (where RAWD data is mapped in memory)
0x1C    4     Memory size (code + data + BSS, >= data size on disk)
              BSS size = memory_size - data_size
```

Example from `qiye.app`:

| Field          | Value          | Notes                              |
|----------------|----------------|------------------------------------|
| Data offset    | `0x970`        | RAWD data starts here in file      |
| Data size      | `0x13A2E0`     | 1,286,880 bytes (~1.2 MB)          |
| Entry point    | `0x80A000A0`   | CRT init entry (within loaded seg) |
| Load address   | `0x80A00000`   | RAWD data loaded here in RAM       |
| Memory size    | `0x144880`     | 1,329,280 bytes                    |
| BSS size       | `0xA5A0`       | 42,400 bytes (zeroed)              |

---

## Import Table

The import table enables dynamic linking against OS/SDK functions. It consists of
three parts: a sub-header, relocation entries, and a name string pool.

### Sub-Header (16 bytes)

```
Offset  Size  Field
------  ----  -----
0x00    4     Entry count
0x04    12    Reserved (zeros)
```

### Import Entry (16 bytes each)

```
Offset  Size  Field
------  ----  -----
0x00    4     Name offset (relative to name string pool start)
0x04    4     Reserved (0)
0x08    4     Relocation type (observed: 0x00020000)
0x0C    4     Target virtual address (where the loader patches)
```

The **target virtual address** identifies the import's stub address within the
code segment. The OS loader resolves the named function and patches all `JAL`
instructions in the code that target this address, rewriting their 26-bit target
field to point to the resolved OS function. The code at the stub address itself
is **not** modified — early imports often overlap with CRT function code
(epilogues, branches), so overwriting them would corrupt the program. The stubs
(typically `NOP; JR $RA`) serve as default no-op implementations when an import
is unresolved.

### Name String Pool

Immediately follows the entries. Null-terminated ASCII strings, packed
sequentially. Name offsets in the entries are relative to the start of this pool.

### Example Imports

| Index | Name                  | Target Address |
|-------|-----------------------|----------------|
| 0     | `abort`               | `0x80A001D0`   |
| 1     | `printf`              | `0x80A001D8`   |
| 5     | `malloc`              | `0x80A001F8`   |
| 7     | `free`                | `0x80A00208`   |
| 11    | `LcdGetDisMode`       | `0x80A00228`   |
| 21    | `lcd_flip`            | `0x80A00278`   |
| 31    | `_kbd_get_key`        | `0x80A002C8`   |
| 63    | `OSTaskCreate`        | `0x80A003C8`   |

Full import categories:

- **C stdlib**: abort, printf, sprintf, fprintf, strlen, malloc, realloc, free,
  fread, fwrite, fseek, strncasecmp
- **LCD/Display**: LcdGetDisMode, \_lcd\_set\_frame, \_lcd\_get\_frame,
  lcd\_get\_cframe, ap\_lcd\_set\_frame, lcd\_flip
- **Filesystem**: fsys\_fopen, fsys\_fread, fsys\_fwrite, fsys\_fclose,
  fsys\_fseek, fsys\_ftell, fsys\_remove, fsys\_rename, fsys\_ferror,
  fsys\_feof, fsys\_findfirst, fsys\_findnext, fsys\_findclose,
  fsys\_flush\_cache, fsys\_RefreshCache, fsys\_fopenW
- **Audio**: waveout\_open, waveout\_close, waveout\_close\_at\_once,
  waveout\_set\_volume, waveout\_can\_write, waveout\_write, pcm\_can\_write,
  pcm\_ioctl, HP\_Mute\_sw
- **Input**: \_kbd\_get\_status, \_kbd\_get\_key, get\_game\_vol
- **RTOS (uC/OS-II)**: OSCPUSaveSR, OSCPURestoreSR, OSTimeGet, OSTimeDly,
  OSSemPend, OSSemPost, OSSemCreate, OSSemDel, OSTaskCreate, OSTaskDel
- **System**: vxGoHome, StartSwTimer, free\_irq, GetTickCount,
  \_sys\_judge\_event, USB\_Connect, USB\_No\_Connect, udc\_attached,
  TaskMediaFunStop, serial\_getc, serial\_putc
- **Cache**: \_\_icache\_invalidate\_all, \_\_dcache\_writeback\_all
- **Unicode**: \_\_to\_unicode\_le, \_\_to\_locale\_ansi, get\_current\_language

---

## Export Table

Same structure as the import table: sub-header + entries + names.

### Export Entry (16 bytes each)

```
Offset  Size  Field
------  ----  -----
0x00    4     Name offset (relative to name string pool start)
0x04    4     Reserved (0)
0x08    4     Type/flags (observed: 0x00020000)
0x0C    4     Virtual address of the exported symbol
```

### Standard Exports

| Name      | Address        | Purpose                                |
|-----------|----------------|----------------------------------------|
| `getext`  | `0x80A0018C`   | Text/localization getter function      |
| `AppMain` | `0x80A001A4`   | RTOS task entry (called via OSTaskCreate) |

`AppMain` is the RTOS task entry point. The OS does **not** call it directly.
Instead, after running the CRT entry point (from the RAWD header), the OS
creates an RTOS task via `OSTaskCreate(AppMain, ...)` which schedules it
for execution with `argc`/`argv` in `$a0`/`$a1`.

---

## Code + Data Segment

Raw MIPS32 (little-endian) machine code. The segment is loaded at the
load address specified in the RAWD header. After loading, the BSS region
(memory\_size - data\_size bytes) following the loaded data is zeroed.

Address translation:

```
file_offset = rawd.data_offset + (virtual_address - rawd.load_address)
virtual_address = rawd.load_address + (file_offset - rawd.data_offset)
```

---

## Resource Pack (Optional)

Appended after the code segment (with 0xFF padding to alignment). Contains an
embedded filesystem of game assets.

### Resource Header

```
Offset  Size  Field
------  ----  -----
0x00    2     Entry count (uint16 LE)
```

### Resource Entry (68 bytes each)

```
Offset  Size  Field
------  ----  -----
0x00    64    File path (null-terminated ASCII, zero-padded)
              Paths use backslash separators, rooted at ".\\"
0x40    4     Cumulative end offset (uint32 LE)
              Relative to the resource section start.
              Resource N data occupies [entry[N-1].end, entry[N].end).
              Entry 0's end offset equals the table size (header + all entries),
              marking the boundary between the directory and the data area.
```

### Data Area

Starts immediately after the last directory entry. Each resource's data is
stored contiguously. The first entry (index 0) serves as a sentinel — its
cumulative offset equals the directory size, so its effective data size is zero.
Actual resource data begins at entry 1.

### Example Resources from `qiye.app`

```
3,580 entries total across these directories:
  common/  (2406 files)  - shared game assets
  audio/   (275 files)   - sound effects and music (.sau format)
  ui/      (271 files)   - UI graphics
  uien/    (260 files)   - English UI localization
  day1-7/  (368 files)   - per-day game content

File types:
  .stx   (1486)  - text/script data
  .soj   (744)   - object/sprite data
  .sai   (545)   - image/animation data
  .sau   (273)   - audio data
  .sst   (217)   - style/theme data
  .sbp   (152)   - bitmap data
  .spl   (123)   - palette data
  .sdt   (19)    - data tables
  .sbn   (18)    - binary data
  .exe   (1)     - embedded executable (WARPlayer.exe)
  .xls   (1)     - spreadsheet data
```

---

## Loading Process

1. Read CCDL header, verify magic and version
2. Parse IMPT, EXPT, RAWD section headers
3. Allocate memory at load address, size = `memory_size`
4. Read code+data from file into memory at load address
5. Zero the BSS region (`load_address + data_size` to `load_address + memory_size`)
6. For each import entry: resolve the named OS function and patch all `JAL`
   instructions targeting that stub address to jump to the resolved function
7. Flush instruction cache (`__icache_invalidate_all`)
8. Call CRT entry point (from RAWD header) — performs app-level init
9. Create RTOS task: `OSTaskCreate(AppMain, argc_argv, stack, priority)`

---

## Platform Details

- **CPU**: MIPS32 little-endian (Actions Semiconductor / ChinaChip SoC)
- **OS**: uC/OS-II RTOS
- **Display**: 320x240 LCD (managed via lcd\_flip / framebuffer APIs)
- **Memory map**: Code loaded at `0x80A00000` region (KSEG0, cached)
- **Devices**: Dingoo A320, A330, and compatible handhelds
