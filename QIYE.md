# 七夜 (Seven Nights) — Game Engine Analysis

Signature: `DingooGames2006 7days`

## Engine: s3d Engine (Simple 3D)

Full 3D action-adventure game built on a custom C++ engine called **s3d**,
targeting the Dingoo A320 (320x240, MIPS32, 336MHz, 32MB RAM).

## Binary Layout

| Section | Address Range | Size |
|---------|--------------|------|
| `.text` | `0x80a00000` – `0x80b3a380` | ~1.23 MB |
| `.bss` | `0x80b3a380` – `0x80b44920` | ~41 KB |

Entry point: `AppMain` at `0x80a001a4`

## Architecture

```
┌─────────────────────────────────────────────────┐
│                  GameEngine                      │
│  (top-level: init, load days, game loop)         │
├──────────┬──────────┬───────────┬────────────────┤
│  Scene   │ Renderer │  Audio    │  GameUnitMgr   │
│  (BSP    │ (soft-   │ (waveout) │  (entity       │
│   world) │  ware)   │           │   management)  │
├──────────┼──────────┼───────────┼────────────────┤
│  s3dBase │ s3dVideo │ s3dTimer  │ s3dIO/FilePak  │
│  s3dLog  │ s3dExit  │ s3dMemInfo│                │
└──────────┴──────────┴───────────┴────────────────┘
```

## Entry Flow

```
getext → AppMain (CRT init, zero BSS) → sub_80a00144 → ...
  → s3d Engine launch → GameEngine::Init → GameEngine::Load (phases 2-4)
  → Begin Game Normal → LoadDay 1 → main game loop
```

Key functions:
- `sub_80a15700` — s3d Engine launcher (refs "s3d Engine launch start")
- `sub_80a45524` — Day/scene loader (loads BSP scenes from `.\day1\` etc.)
- `sub_80a57ba8` / `sub_80a57e88` — Renderer DrawPrimitive (triangle list/strip)
- `sub_80a47b30` / `sub_80a47a90` — Audio task creation (waveout init + OS task)
- `sub_80a4780c` — Audio playback task (calls `waveout_write`)
- `sub_80a000d8` — Input polling wrapper (calls `_kbd_get_key`)

## Subsystems

### 1. Software 3D Renderer (`Renderer`, `Raster`)

Full software rasterizer targeting 320x240 LCD.

- Vertex/index buffer pipeline with triangle lists & strips
- **Shaders** (all software): `BasicShader`, `FogShader`, `FadeShader`,
  `GrayShader`, `InkShader`, `TwistShader`, `BlindShader`, `MotionBlurShader`
- Post-effects: `Lensflare`, `FakeShadow`
- Texture management: `Texture`, `TextureRes`, `ResManager`

RTTI classes: `Renderer` (0x80adc868), `Raster` (0x80adc7e8),
`Renderable` (0x80ad8a9c)

### 2. BSP World Engine (`Bsp`, `Scene`)

Quake-style BSP for 3D levels with custom format (not standard Quake).

BSP pipeline lumps:
- Vertex, Edge, FaceEdge, Plane, Texinfo
- Lightmap, LightmapStyles, VertLights (+ dynamic lighting)
- Face, Brush, BrushSide, VisBrush, VisFace
- Leaf, LeafBrush, LeafVBrush, Node
- PVS (Potentially Visible Set)
- Model, EPair, EBrush, ELink, Ent, VisEntIdx, EDictIdx
- PathPairs, PathNodes (AI navigation)

Debug strings at `0x80b321a0`–`0x80b32964` (Bsp::LoadFromFile series).

### 3. Game Entity System (`GameUnit`, `GameUnitManager`, `Object`)

- `Renderable` — base for visible objects
- `PhysicUnit` — physics component
- `BspUnit` — world-integrated entity
- `BillboardGroup` — sprites/particles
- `VisUnit` — visibility culling
- `Things` — generic interactive objects
- `Pickable`, `Pushable`, `Breakable`

RTTI: `GameUnit` (0x80ad88c0), `GameUnitManager` (0x80ade048),
`MyGameUnitMgr` (0x80ad9f0c), `Object` (0x80ad9470)

### 4. Character System (`Player`, `StateMachine<Player>`)

Player state machine with full combo combat:

| State | RTTI |
|-------|------|
| `CStand` | `N14PlayerStateSet6CStandE` |
| `CRun` | `N14PlayerStateSet4CRunE` |
| `CHide` | `N14PlayerStateSet5CHideE` |
| `CPush` | `N14PlayerStateSet5CPushE` |
| `CFPS` | `N14PlayerStateSet4CFPSE` (first-person mode?) |
| `CAttackA1`–`A3` | 3-hit combo chain A |
| `CAttackB1`–`B3` | 3-hit combo chain B |
| `CAttackC1`–`C3` | 3-hit combo chain C |
| `CHurt`–`CHurt4` | 4 damage levels |
| `CDie` | Death |
| `CQTE` | Quick Time Event |

NPCs/Enemies:
- `Enemy`, `Creature`, `Bully` (Day3/Day5 variants), `Ghost`
- `FatMaid`, `Maid`/`Maid2`/`Maid3`, `Lord`, `DarkKen`
- `Victor`, `Kate`, `Sam`, `Prisoner`, `Lamper`, `Worm`, `Weed`

Weapons: `Sword`, `GunFire`, `ElectricSaw`

Animation naming: `r_<char>_<action>_<variant>` (render),
`c_<char>_<action>` (collision), `ow_<char>_<action>` (overworld?)

### 5. Trigger System

Event-driven gameplay via trigger volumes:

- `ActTrigger` — generic action
- `DoorTrigger` — doors (18+ named: `oscj_door_aa` through `oscj_door_ra`)
- `CameraTrigger` / `CameraSpot` — cinematic camera control
- `EventTrigger` — scripted events
- `ItemTrigger` / `PickTrigger` — item interaction
- `HideTrigger` — hide mechanics
- `TalkTrigger` — NPC dialog
- `LightTrigger` — dynamic lighting events
- `DummySpot` — waypoints
- `StayBox`, `Trap` — gameplay zones

### 6. Scripting (`SimpleScript`, `ScriptInterface`)

- Scripts in `.sbp` files loaded from PAK archives
- `SimpleScript::LoadFromPak` / `SimpleScript::LoadArch`
- Script commands control camera, player, game state
- Debug: `##LoadNextDay`, `##PausePlayer`, `##SetPlayerClip`,
  `##FreezeCameraTrigger`

### 7. Dialog/UI System

- `DialogManager` → `Dialog` (in-game conversations)
- UI dialogs: `MainUIDlg`, `PlayDlg`, `HelpDlg`, `AboutDlg`,
  `LoadDlg`, `OptionDlg`, `GameUIDlg` (HUD)
- `Font`, `StringManager` — text rendering
- Localization: `basefont.sbn` (EN: `.\uien\`, CN: `.\ui\`),
  `get_current_language`, `__to_unicode_le`, `__to_locale_ansi`

### 8. Resource System

- `s3dFilePak` — PAK archive format for bundled assets
- `ResManager` → `Resource`, `TextureRes`, `ObjectRes`
- Stream hierarchy: `InputStream`/`OutputStream`,
  `InputFileStream`/`OutputFileStream`,
  `InputDataStream`/`OutputDataStream`

### 9. Audio (`AudioData`)

- PCM playback via `waveout_open/write/close`
- Sample rate: 16000 Hz (`0x3e80`), 16-bit (`0x10`)
- Double-buffered: 0x640 (1600) + 0x320 (800) byte buffers
- Runs as separate uC/OS-II task (priority 0x10+)
- Semaphore-protected buffer access (`OSSemCreate(1)` × 2)
- Volume: `waveout_set_volume`, `HP_Mute_sw`

### 10. Networking (likely unused)

- `ClientSession`, `MyClientSession`, `TcpSession`,
  `HttpSession`, `SessionDevice`

## Game Structure

- **13 days** (not 7 as the title implies): `LoadDay 1` through `LoadDay 13`
- Each day loads scenes from `.\day1\` etc. directories
- Scenes are BSP maps with `.sbp` script files
- Day progression via `##LoadNextDay`
- Day 5 has branching: `_1f_day5`, `1f_day5`, `2f_day5`, `3f_day5`
- Save system: `slot%d.sav`
- Game state: `.\common\default.s3dsty`, `.\ui\state.sdt`
- Game over: `m_gameover`

## File Formats

| Extension | Purpose |
|-----------|---------|
| `.sbp` | SimpleScript bytecode (script packs) |
| `.sbn` | Font/string binary data |
| `.sdt` | State data tables |
| `.s3dsty` | s3d engine style/config |
| `.sav` | Save game slots |
| BSP (custom) | 3D level: geometry, lightmaps, PVS, entities, nav mesh |

## RTTI Class Hierarchy (from mangled names)

```
s3dBase
├── s3dIO
│   └── s3dFilePak
├── s3dLog
├── s3dTimer
├── s3dExit
├── s3dVideo
│   └── VID_impl
└── s3dMemInfo

Engine
├── Renderer
├── Raster
└── GameEngine

Object
├── GameUnit
│   ├── BspUnit
│   ├── Player (StateMachine<Player>)
│   ├── Enemy / Creature
│   ├── Bully / BullyDay3 / BullyDay5
│   ├── Ghost / DarkKen / Lord / Victor
│   ├── Kate / Sam / Prisoner
│   ├── Maid / Maid2 / Maid3 / FatMaid
│   ├── Lamper / Worm / Weed
│   ├── Sword / GunFire / ElectricSaw
│   ├── Pickable / Pushable / Breakable
│   └── Trap / StayBox
├── Scene
├── Bsp
├── Animation
├── AudioData
├── Texture
├── Font
└── Dialog

MsgReceiverInterface
└── MsgReceiver

Renderable
├── BillboardGroup
├── VisUnit
├── FakeShadow
└── Lensflare

Resource
├── TextureRes
└── ObjectRes

Stream
├── InputStream → InputFileStream, InputDataStream
└── OutputStream → OutputFileStream, OutputDataStream

SessionDevice
├── TcpSession
├── HttpSession
└── ClientSession → MyClientSession

ScriptInterface
└── SimpleScript

GameUnitManager → MyGameUnitMgr
DialogManager
StringManager
ResManager

PlayerStateSet (StateSet<Player>)
├── CStand, CRun, CHide, CPush, CFPS
├── CAttackA1–A3, CAttackB1–B3, CAttackC1–C3
├── CHurt, CHurt2, CHurt3, CHurt4
├── CDie, CQTE
└── State<Player>

Triggers: ActTrigger, DoorTrigger, CameraTrigger,
  EventTrigger, ItemTrigger, PickTrigger,
  HideTrigger, TalkTrigger, LightTrigger

Shaders: BasicShader, FogShader, FadeShader, GrayShader,
  InkShader, TwistShader, BlindShader, MotionBlurShader

Spots: CameraSpot, DummySpot
```

## SDK Surface (77 imports)

See CLAUDE.md for the full import table. All SDK calls go through the
CCDL trampoline table at `0x80a001d0`–`0x80a00408`.
