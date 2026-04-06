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

## Entry Flow (verified via decompilation)

```
_start(0x80a000a0)
  ├── a1=0: zero BSS (0x80b4a2e0..0x80b44880), alloc framebuffer (320×240×2 = 0x25800)
  └── a1=1: free framebuffer

AppMain(0x80a001a4) → s3d_main(0x80a00700)
  ├── set resolution 320×240 (0x140, 0xf0)
  ├── extract base path from argv
  ├── new GameEngine (0xab7c bytes)
  │   ├── Engine_ctor: creates Renderer(0x9c), Scene(0x6348)
  │   │   DialogManager(0xcc), Font, StringManager, ResManager, Raster
  │   ├── GameEngine fields: GameSI, GameUIDlg, SimpleScript state
  │   └── vtable at 0x80ad9980
  ├── vtable[3] → GameEngine::Load (phases 2-4)
  ├── Engine_init → subsystem init
  ├── vtable[5] → first Engine_tick (returns frame_result)
  └── main loop:
      while (_sys_judge_event(0, frame_result) >= 0):
        input_dispatch(engine, frame_result)  // kbd → game input mapping
        frame_result = Engine_tick(engine)     // update + render frame
      vtable[7] → shutdown
      vtable[1] → destructor
```

### Rename Statistics

All 3113 functions named — zero `sub_*` remaining (100% coverage).

### Key functions (renamed in Binary Ninja)

**Core Engine:**

| Address | Name | Description |
|---------|------|-------------|
| `0x80a00700` | `s3d_main` | Top-level: create engine, run loop, shutdown |
| `0x80a15cd0` | `GameEngine_ctor` | Constructor (0xab7c bytes, vtable 0x80ad9980) |
| `0x80a15e48` | `GameEngine_dtor` | Destructor, cleans up subsystems |
| `0x80a102c0` | `Engine_ctor` | Base Engine init: Renderer, Scene, audio, I/O |
| `0x80a3c0f0` | `Scene_ctor` | Scene constructor (0x6348 bytes) |
| `0x80a8c458` | `DialogManager_ctor` | Dialog system init (0xcc bytes) |
| `0x80a10f30` | `Engine_init` | Post-construction subsystem initialization |
| `0x80a110c0` | `Engine_tick` | Per-frame update: timer → logic → physics → render |
| `0x80a0046c` | `input_dispatch` | Map kbd bitmask → game actions (13 buttons) |
| `0x80a114fc` | `Engine_keyDown` | Key press handler with repeat detection |
| `0x80a115c4` | `Engine_keyUp` | Key release handler |
| `0x80a3d2f0` | `Scene_loadDay` | Load day N: BSP map, scripts, entities |
| `0x80a3dffc` | `Scene_update` | Scene per-frame: state machine, transitions, audio |
| `0x80a3d4fc` | `Scene_unloadCurrent` | Unload current scene/episode |
| `0x80a3d538` | `Scene_loadEpisode` | Load episode within current day |
| `0x80a3d674` | `Scene_setupCamera` | Position camera for scene |
| `0x80a1a9c4` | `GameEngine_loadNextDay` | Advance to next day (day index + 1) |
| `0x80a16768` | `GameEngine_saveSaveFile` | AES-encrypted save to `slot%d.sav` |
| `0x80a15f58` | `GameEngine_initUI` | Load strings, fonts, create HUD/dialogs |

**GameEngine UI & Menus:**

| Address | Name | Description |
|---------|------|-------------|
| `0x80a201dc` | `GameEngine_loadUIResources` | Load all UI sprites |
| `0x80a20d38` | `GameEngine_updateMenuState` | Main menu state machine (states 0-14) |
| `0x80a22f60` | `GameEngine_updateSaveLoadMenu` | Save/load slot selection |
| `0x80a23300` | `GameEngine_updateHealthBar` | Health bar state update |
| `0x80a234b0` | `GameEngine_renderQTE` | Quick time event rendering |
| `0x80a23a68` | `GameEngine_renderMapView` | Floor map rendering with scroll |
| `0x80a25a60` | `GameEngine_updatePasswordMenu` | Password entry with d-pad |
| `0x80a25da4` | `GameEngine_renderCutsceneImage` | Fullscreen cutscene image |
| `0x80a26338` | `GameEngine_renderDialog` | Dialog box rendering |
| `0x80a26d48` | `GameEngine_render3DMapPreview` | 3D room preview on map |
| `0x80a27558` | `GameEngine_renderNumber` | Digit-by-digit number rendering |
| `0x80a2822c` | `GameEngine_renderProgressBar` | Progress/loading bar |
| `0x80a285a4` | `GameEngine_renderLoadingScreen` | Loading screen display |

**DialogManager:**

| Address | Name | Description |
|---------|------|-------------|
| `0x80a25ec0` | `DialogManager_showDialog` | Show basic dialog |
| `0x80a25f6c` | `DialogManager_showDialogWithChoice` | Dialog with choice selection |
| `0x80a26004` | `DialogManager_showAutoDialog` | Auto-scrolling dialog |
| `0x80a260bc` | `DialogManager_showStandardMsg` | Standard message display |
| `0x80a26200` | `DialogManager_getResult` | Get dialog result |
| `0x80a27688` | `DialogManager_reset` | Reset dialog state |

**Scene & Level Management:**

| Address | Name | Description |
|---------|------|-------------|
| `0x80a314cc` | `Scene_createEnemyTypes` | Create all enemy type objects |
| `0x80a31bcc` | `Scene_loadEnemyModels` | Load enemy models per floor |
| `0x80a33038` | `Scene_createEntity` | Create entity by type |
| `0x80a33860` | `Scene_spawnEnemy` | Spawn enemy with collision check |
| `0x80a34944` | `Scene_spawnAnimatedEntities` | Spawn entities from anim data |
| `0x80a34ecc` | `Scene_saveToStream` | Save scene state to stream |
| `0x80a351b4` | `Scene_loadEntities` | Load all entities from stream |
| `0x80a35c24` | `Scene_updateDoorInteraction` | Update player-door interaction |
| `0x80a36720` | `Scene_renderSkybox` | Render skybox with transform |
| `0x80a37518` | `Scene_updatePlayerMovement` | Update player movement/velocity |
| `0x80a38750` | `Scene_loadPlayerAdult` | Load adult player model (ken) |
| `0x80a384d0` | `Scene_loadPlayerChild` | Load child player model (xiaoken) |
| `0x80a3937c` | `Scene_findNearestEnemy` | Find closest attackable enemy |
| `0x80a39f6c` | `Scene_updateEnemyAI` | Update enemy AI behavior |

**GameData (Save System):**

| Address | Name | Description |
|---------|------|-------------|
| `0x80a16988` | `GameEngine_loadSaveFile` | AES+MD5 save file loading |
| `0x80a16c90` | `GameEngine_loadAllSaveSlots` | Loads 3 save slots |
| `0x80a177d8` | `GameEngine_writeSaveSlot` | Write save to slot file |
| `0x80a30150` | `GameData_initSlotData` | Init save slot data |
| `0x80a3026c` | `GameData_loadSlotData` | Load save slot from stream |
| `0x80a308ac` | `GameData_saveSlotData` | Save slot to stream |
| `0x80a3ffb8` | `GameEngine_saveGame` | Full game save (encrypt+write) |

**Title Screen:**

| Address | Name | Description |
|---------|------|-------------|
| `0x80a29fd4` | `TitleScreen_update` | Title/main menu update loop |
| `0x80a2a614` | `TitleScreen_updatePasswordEntry` | Password entry on title |
| `0x80a2aaa4` | `TitleScreen_updateLoadMenu` | Load save menu on title |

**Script System:**

| Address | Name | Description |
|---------|------|-------------|
| `0x80a17a90` | `ScriptTable_init` | Registers ~200 script commands |
| `0x80a9b2ec` | `SimpleScript_loadFromSST` | Load `.sst` script archive |
| `0x80a1a40c` | `Script_WaitFrames` | Wait N frames |
| `0x80a1a464` | `Script_WaitForDialog` | Wait for dialog completion |
| `0x80a1a51c` | `Script_ShowDialog` | Show dialog box |
| `0x80a1a7e4` | `Script_SetFogColorTop` | Set top fog color (RGB565) |
| `0x80a1aa20` | `Script_SetupCamera` | Set camera position/target/color |
| `0x80a1aea0` | `Script_CreateEmitter` | Create particle emitter |
| `0x80a1b14c` | `Script_SpawnEntity` | Spawn game entity |
| `0x80a1b98c` | `Script_SetLightColor` | Set scene light color |
| `0x80a1be18` | `Script_SetCameraShake` | Set camera shake effect |
| `0x80a1c148` | `Script_SetPlayerPosition` | Set player world position |
| `0x80a1c7cc` | `Script_CheckAllEnemiesDead` | Check if all enemies killed |
| `0x80a1cecc` | `Script_PlayAnimation` | Play entity animation |
| `0x80a1dbe0` | `Script_SetEntityAI` | Configure entity AI behavior |
| `0x80a1e158` | `Script_SpawnProjectile` | Spawn projectile entity |

**GameUnit (Model/Mesh/Skeleton):**

| Address | Name | Description |
|---------|------|-------------|
| `0x80a11a94` | `GameUnit_ctor` | GameUnit base constructor |
| `0x80a11e2c` | `GameUnit_loadModel` | Load 3D model + create material slots |
| `0x80a12204` | `GameUnit_loadMaterials` | Load material textures |
| `0x80a141e4` | `GameUnit_renderModel` | Standard model rendering |
| `0x80a146cc` | `GameUnit_renderModelLit` | Lit model rendering |
| `0x80a14c10` | `GameUnit_renderModelShadow` | Shadow pass rendering |
| `0x80a14e9c` | `GameUnit_updateSkeletonMatrices` | Bone matrix computation |
| `0x80a15960` | `ResFactory_loadResource` | Load resource into factory |

**Camera System:**

| Address | Name | Description |
|---------|------|-------------|
| `0x80ab4500` | `Camera_setMode` | Switch camera mode (cases 0-8) |
| `0x80aab9ac` | `Camera_attachEntity` | Set entity ref + copy position |
| `0x80aabf68` | `Camera_restoreFromTrigger` | Restore camera from trigger stack |
| `0x80aaba84` | `Camera_setLimits` | Set angle limit fields |
| `0x80aab854` | `Camera_getForwardDir` | Compute normalized forward |

**Animation & Audio:**

| Address | Name | Description |
|---------|------|-------------|
| `0x80a75e30` | `AnimRes_ctor` | AnimRes constructor (`.sai` format) |
| `0x80a763fc` | `AnimRes_loadFromMemory` | Parse .sai: bones, keyframes, vertex anim |
| `0x80a76a30` | `AnimRes_sampleBoneMatrix` | Interpolate bone matrix at time t |
| `0x80a77680` | `AudioData_ctor` | AudioData constructor (RTTI "9AudioData") |
| `0x80a77a18` | `AudioData_decompressADPCM` | ADPCM decompression (3-bit codes, step table) |
| `0x80a77b00` | `SoundManager_init` | Init sound manager, create AudioDevice |
| `0x80a77dcc` | `SoundManager_assignChannel` | Assign sound to channel by priority |
| `0x80a77e94` | `SoundManager_update` | Update channels, handle BGM looping/fade |
| `0x80a47980` | `AudioDevice_init` | Create device, open waveout, spawn task |
| `0x80a47770` | `AudioDevice_taskMain` | OS task: 16-ch mixer → waveout_write |

**Matrix Math (16.16 fixed-point):**

| Address | Name | Description |
|---------|------|-------------|
| `0x80a639b4` | `mat4_adjugate` | 4x4 cofactor/adjugate matrix |
| `0x80a64e94` | `mat4_inverse` | Full 4x4 inverse |
| `0x80a6b040` | `mat4_rotateX` | X rotation in-place |
| `0x80a6c994` | `mat4_rotateEuler` | Euler rotation on existing matrix |
| `0x80a70620` | `mat4_fromQuaternion` | Quaternion to matrix |
| `0x80a72220` | `mat4_lookAt` | LookAt view matrix (LH) |
| `0x80a725b0` | `mat4_frustum` | Frustum projection (LH) |
| `0x80a73400` | `mat4_perspectiveFov` | Perspective from FOV (LH) |
| `0x80a74480` | `mat4_shadow` | Shadow projection onto plane |

**Spline System:**

| Address | Name | Description |
|---------|------|-------------|
| `0x80aa0384` | `Spline_evaluate` | Bezier/B-spline evaluation |
| `0x80aa1ed0` | `Spline_subdivide` | Spline subdivision/refinement |
| `0x80aa27c4` | `Spline_computeArcLength` | Arc-length lookup table (softfloat) |
| `0x80aa2b10` | `Spline_evalPosition` | Evaluate position at parameter t |
| `0x80aa2cdc` | `Spline_evalTangent` | Evaluate tangent at parameter t |
| `0x80aa391c` | `Spline_computeBounds` | Compute AABB bounds |

**Enemy & Creature AI:**

| Address | Name | Description |
|---------|------|-------------|
| `0x80aa4d00` | `Bully_ctor` | Bully enemy constructor (type 2) |
| `0x80aa56fc` | `Bully_update` | Main update: AI + Creature_update |
| `0x80aa5814` | `Bully_decideAction` | Weighted random AI decision |
| `0x80aa8500` | `BullyDay3_ctor` | BullyDay3 boss constructor (type 0xd) |
| `0x80aa90ec` | `Enemy_decideAction` | Randomized attack/walk/idle selection |
| `0x80abe52c` | `Creature_stateChase` | Chase target, random attack decision |
| `0x80ab6b7c` | `DarkKen_ctor` | DarkKen enemy (2 sub-models) |
| `0x80ac0d60` | `Ghost_update` | Ghost per-frame update |
| `0x80ac1550` | `Lamper_think` | Lamper AI think routine |

**Player State Machine:**

| Address | Name | Description |
|---------|------|-------------|
| `0x80ac960c` | `PlayerState_Stand_getTransition` | Stand transition check |
| `0x80ac9bd4` | `PlayerState_AttackA1_enter` | AttackA1 combo start |
| `0x80acb3b8` | `PlayerState_Push_enter` | Push state (pushable interaction) |
| `0x80acbf74` | `PlayerState_FPS_enter` | FPS mode enter |
| `0x80acc198` | `PlayerState_QTE_enter` | QTE enter |
| `0x80acc2d0` | `PlayerState_Hide_getTransition` | Hide transition |
| `0x80adad94` | `StateMachine_enterState` | Enter state machine state |
| `0x80adae3c` | `StateMachine_update` | Update state machine |

**Software Rasterizer:**

| Address | Name | Description |
|---------|------|-------------|
| `0x80acc4c0` | `Raster_clipPolygon` | Clip polygon to frustum |
| `0x80acdda8` | `Raster_transformAndClipPolygon` | Full transform + clip pipeline |
| `0x80adc554` | `Raster_selectScanlineFunc` | Select scanline rasterizer |
| `0x80adc5f0` | `Raster_blendPixel` | Blend RGB565 pixel |
| (0x80a4efb0+) | `Raster_scanline_*` | 160+ scanline variants (z×tex×key×blend) |

**Crypto & Hashing:**

| Address | Name | Description |
|---------|------|-------------|
| `0x80aba708` | `aes_decrypt` | AES-128-CBC decrypt |
| `0x80aba798` | `aes_encrypt` | AES-128-CBC encrypt |
| `0x80a2ea2c` | `md5_compute` | MD5 hash |

**C++ Runtime & Memory:**

| Address | Name | Description |
|---------|------|-------------|
| `0x80acf6b0` | `operator_new` | C++ `new` — malloc + bad_alloc throw |
| `0x80acf660` | `operator_delete` | C++ `delete` |
| `0x80ad07b4` | `__dynamic_cast` | C++ dynamic_cast runtime |
| `0x80ad11c4` | `__gxx_personality_v0` | GCC personality routine |
| `0x80ad191c` | `__cxa_throw` | Throw C++ exception |
| `0x80ad533c` | `_Unwind_RaiseException` | Raise C++ exception |
| `0x80ad9110` | `HashMap_ctor` | HashMap constructor |
| `0x80ad950c` | `HashMap_insert` | HashMap key insertion |
| `0x80ad93d8` | `HashMap_lookup` | HashMap key lookup |

**Data Structures:**

| Address | Name | Description |
|---------|------|-------------|
| `0x80ad9610` | `DynArray_init` | DynArray init (generic) |
| `0x80ad9804` | `DynArray_pushBack` | DynArray append element |
| `0x80ad9878` | `DynArray_resize` | DynArray resize buffer |
| `0x80ad9a0c` | `Stream_writeInt32` | Write 32-bit int to stream |
| `0x80ade868` | `Stream_writeString` | Write string to stream |

**I/O Classes:**

| Address | Name | Description |
|---------|------|-------------|
| `0x80a8cc48` | `StringManager_loadFromFile` | Load str.sdt from PAK |
| `0x80a8ce34` | `StringManager_parseStrTable` | Parse string table binary |

### Engine_tick frame pipeline

```
Engine_tick(engine):
  1. update timer (fixed-point delta)
  2. if (engine.update_enabled):  vtable[0x24] → logic update
  3. Engine_updateSubsystems()    // physics, triggers, AI
  4. if (engine.render_enabled):  vtable[0x34] → render scene
  5. if (engine.render2d_enabled): vtable[0x30] → render UI overlay
  6. Engine_postRender()          // buffer swap, effects
  7. if (engine.audio_enabled):   vtable[0x28] → audio update
  8. Renderer::Present()          // blit to LCD
  9. Font::Flush()
```

### Input mapping (input_dispatch)

Reads `_kbd_get_status` bitmask and maps to engine input events:

| Bit mask | Action ID | Likely button |
|----------|-----------|---------------|
| `0x10000000` | 0x0D | A (confirm) |
| `0x40000` | 0x0E | B (cancel) |
| `0x100000` | 0x0F | X (attack?) |
| `0x8000000` | 0x10 | Y |
| `0x200000` | 0x12 | Select |
| `0x10000` | 0x11 | Start |
| `0x80000000` | 0x08 | D-pad (combined) |
| `0x40` | 0x02 | Shoulder? |

## Subsystems

### 1. Software 3D Renderer (`Renderer`, `Raster`)

Full software rasterizer targeting 320x240 LCD. Renderer object size `0x1a388`.

- Vertex/index buffer pipeline with triangle lists & strips
- **Shaders** (all software): `BasicShader`, `FogShader`, `FadeShader`,
  `GrayShader`, `InkShader`, `TwistShader`, `BlindShader`, `MotionBlurShader`
- Post-effects: `Lensflare`, `FakeShadow`, `BloodEffect`
- Texture management: `Texture`, `TextureRes`, `ResManager`
- **Scanline function table**: 26-entry table in `Raster`, combinatorial matrix of:
  - Z-buffer modes: none, z-test, z-write, z-read-write
  - Texture modes: flat fill, textured with palette, textured with palette+fog
  - Color key transparency: with/without key test
  - Blend modes: opaque, blend50, blend75, blend25, lightmap alpha
  - 160+ individual scanline functions at `0x80a4efb0`+
- **DepthBuffer**: Constructor/destructor, fog params, fog-to-buffer application
- **Polygon clipping**: `Raster_clipPolygon`, per-plane clip (left/right/top/bottom/near),
  `Raster_transformAndClipPolygon` (full pipeline)
- **Projection**: `Camera_projectPoint`, `Camera_projectPointClamp`, `Camera_projectSprite`
- All math in 16.16 fixed-point, RGB565 color space

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

GameUnit provides: model loading (`loadModel`, `loadMaterials`), skeleton/bone
matrices (`updateSkeletonMatrices`, `loadSkeleton`), animation playback
(`setAnimation`, `getAnimFrameCount`), rendering (`renderModel`, `renderModelLit`,
`renderModelShadow`), and bounding box management (`updateBoundsTransform`,
`calcBoundsViewDist`).

Query helpers: `isAnimDone`, `isMoving`, `isAttacking`, `isFacingTarget`,
`hasTarget`, `checkDistanceTo`.

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

NPCs/Enemies (full hierarchy from RTTI):
```
Creature (base state machine: Stand/Walk/Idle/Attack/Hurt/Death/Chase)
├── Enemy (decideAction: randomized attack/walk/idle)
│   ├── Bully (entity type 2, vtable 0x80adebb0, 17 anims, 14 audio)
│   │   └── BullyWeapon (sub-entity, vtable 0x80aded68)
│   ├── BullyDay3 (entity type 0xd, vtable 0x80adec98, weapon "ow_dianju_a")
│   ├── BullyDay5 (RTTI "9BullyDay5")
│   ├── Ghost (5 zones, sub-models, weapon "ow_shuiguo_a")
│   ├── DarkKen (2 sub-models, 3 anim tracks)
│   ├── Lamper (proximity AI, trigger zones, damage check)
│   ├── Weed (states: Stand/Walk/Attack, spore shooting)
│   └── Worm
├── Lord (boss, extended serialize/deserialize)
├── Victor, Kate, Sam, Prisoner
├── FatMaid (entity type 3), Maid/Maid2/Maid3
└── ElectricSaw (RTTI "11ElectricSaw")
```

Weapons: `Sword`, `GunFire` (constructor at 0x80adaaac), `ElectricSaw`

Blood effects: `BloodEffect_loadResources`, `BloodEffect_spawn`, `BloodEffect_update`, `BloodEffect_render`

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

### 5b. Camera System (`Camera`, `CameraSpot`)

Camera object with 7+ modes and full 3D collision:

| Mode | Description |
|------|-------------|
| 0 | Follow (default third-person) |
| 1 | Orbit |
| 2 | Chase |
| 3 | Static |
| 4 | Free |
| 5 | Cinematic |
| 6 | Path (spline-based) |
| 7-8 | Additional modes |

Key features:
- Entity attachment (`Camera_attachEntity`)
- Camera shake (frequency + amplitude)
- Angle limits
- Collision detection with BSP
- Trigger-based camera save/restore stack
- Input manager and render data references
- `CameraSpot`: Spline-based camera paths (vtable 0x80adf168)
- `CameraTrigger`: Zone-based camera switching (add/remove/find/update zones)

### 5c. Spline System (`Spline`, `SplineCurve`)

Full Bezier/B-spline system for camera paths and entity motion:

- `Spline_evaluate/evaluateB/evaluateC` — 3 evaluation variants
- `Spline_subdivide` — spline subdivision/refinement
- `Spline_computeArcLength` — cumulative arc-length lookup table (uses softfloat)
- `Spline_evalPosition/evalTangent` — position and tangent at parameter t
- `Spline_moveControlPoint` — control point + tangent handle adjustment
- `Spline_translate/transform/computeBounds` — spatial operations
- `SplineCurve_alloc/dtor/copyFrom` — curve memory management

### 6. Scripting (`SimpleScript`, `ScriptInterface`)

- Scripts in `.sbp` files loaded from PAK archives
- `SimpleScript::LoadFromPak` / `SimpleScript::LoadArch`
- Script commands control camera, player, game state
- Debug: `##LoadNextDay`, `##PausePlayer`, `##SetPlayerClip`,
  `##FreezeCameraTrigger`

#### SST Script Archive Format (from `SimpleScript_loadFromSST`)

`.sst` files are typed node trees loaded by `SimpleScript_loadFromSST`:

```
Header:
  u16  node_count         // number of typed script nodes

For each node:
  u16  node_type           // 0-5, selects constructor:
       0 → ScriptInterface::ctor_type0
       1 → ScriptInterface::ctor_type1
       2 → GameSI::ctor
       3 → ScriptInterface::ctor_type3 (with error log path)
       4 → GameSI::ctor_alt
       5 → SimpleScript::ctor

After nodes:
  u16  command_count       // script command entries
  <commands loaded via sub_80a9c068>

  u16  subroutine_count    // script subroutines
  For each: loaded via sub_80a9c068
```

### 7. Dialog/UI System (verified via decompilation)

- `DialogManager` → `Dialog` (in-game conversations)
- UI dialogs: `MainUIDlg`, `PlayDlg`, `HelpDlg`, `AboutDlg`,
  `LoadDlg`, `OptionDlg`, `GameUIDlg` (HUD)
- `Font`, `StringManager` — text rendering
- Localization: `basefont.sbn` (EN: `.\uien\`, CN: `.\ui\`),
  `get_current_language`, `__to_unicode_le`, `__to_locale_ansi`

#### DialogManager structure (0xcc bytes)

```c
struct DialogManager {  // vtable at 0x80adda68
    void**  vtable;           // +0x00
    u8      padding[0x14];    // +0x04  (zeroed)
    void*   field_18;         // +0x18
    // ... vtable2 at +0x18 (0x80adda84)
    u32     field_38;         // +0x38  (0 = no dialog active)
    u32     field_3c;         // +0x3c
    u32     field_44;         // +0x44  (dialog linked list head)
    DlgArray  pages[0x20×0xa]; // +0x4c (dynamic array, 0x20 cap, 0xa pages)
    // resource refs at +0x84, +0x94, +0x98, +0xa4
    u32     screen_w;         // +0x30  from resolution
    u32     screen_h;         // +0x34
    u32     screen_d;         // +0x38
    // linked list ptrs at +0x60..+0x80 (8 layers × pages × items)
};
```

Dialog layers (8 pointer arrays at offsets 0x60–0x80):
textures, sprites, labels, buttons, checkboxes, sliders, containers, items

#### StringManager (`str.sdt` format)

`StringManager_loadFromFile` (0x80a8cc48) → `StringManager_parseStrTable` (0x80a8ce34):

```
str.sdt binary format:
  u32  string_count
  For each string:
    u16  byte_length          // big-endian
    u8   data[byte_length]    // string bytes (null-terminated after copy)
    u8   padding[0x18 - 2 - byte_length]  // fixed 0x18 stride per entry

  u32  label_count
  For each label:
    u16  byte_length          // big-endian
    u8   data[byte_length]
    ... (same 0x18 stride)
  Labels are copied into 0x40-byte slots for lookup
```

#### UI initialization (`GameEngine_initUI` at 0x80a15f58)

```
1. Detect language (get_current_language → 2=CN, else=EN)
2. Load StringManager from `.\ui[en]\str.sdt`
3. Load font from `.\ui[en]\system.sbn`
4. Create GameUIDlg (0x2ac bytes) — main HUD overlay
5. Create PlayDlg (0x524 bytes) — in-game dialog controller
6. Load state from `.\ui[en]\state.sdt`
7. Init 4 UI state slots (0xa198, 0xa1c4, 0xa1f0, 0xa204)
```

#### UIWidget base class (0x80a89910)

```c
struct UIWidget {  // vtable at 0x80add5a0
    void**  vtable;           // +0x00
    u32     screen_x;         // +0x04 (from parent +0x30)
    u32     screen_y;         // +0x08 (from parent +0x34)
    u32     screen_z;         // +0x0c (from parent +0x38)
    u32     field_10;         // +0x10
    u32     x;                // +0x14
    u32     y;                // +0x18
    u32     width;            // +0x1c
    u32     height;           // +0x20
    u8      visible;          // +0x30
    u8      enabled;          // +0x31 (1=enabled)
    u8      focused;          // +0x32
    u8      interactive;      // +0x33 (1=interactive)
    void*   parent;           // +0x34
    void*   owner;            // +0x38
    void*   dialog_mgr;      // +0x3c
    // linked list: prev +0x50, next +0x54
    // child widgets: 20 slots at +0x17c..+0x21c (6 words each)
    //   each child: [hit_rect_x, hit_rect_y, hit_rect_w, hit_rect_h, color=0xffffd8f1]
};
```

### 8. Resource System

- `s3dFilePak` — PAK archive format for bundled assets
- `ResManager` → `Resource`, `TextureRes`, `ObjectRes`
- `ResFactory` — loads resources by type: 0=sau, 1=soj, 2=sai, 3=stx
- `HashMap` — asset caching (ctor, insert, lookup, remove, clear, hash, iterateAll)
- Stream hierarchy: `InputStream`/`OutputStream`,
  `InputFileStream`/`OutputFileStream`,
  `InputDataStream`/`OutputDataStream`

#### Stream Classes (RTTI confirmed)

```
Stream (0x80ad90d4)
├── InputStream (0x80ad90b8)
│   ├── InputDataStream (0x80ad90a4) — big-endian deserialization
│   │   readByte, readShortBE, readIntBE, readString, readFixedPoint,
│   │   readVec2, readVec3, readVec4, readPlane
│   └── InputFileStream (0x80ad9090) — file reading via fsys_fopen
└── OutputStream
    ├── OutputDataStream — big-endian serialization
    │   writeByte, writeShortBE, writeIntBE, writeString, writeFixedPoint,
    │   writeVec2, writeVec3, writeVec4, writePlane
    └── OutputFileStream — file writing via fsys_fopen
```

#### Data Structures

- **DynArray**: Template-like growable array with variants for 4/28/52-byte elements
  (init, free, resize, pushBack, ensureIndex, getSize, removeAt, setAt)
- **HashMap**: Hash map with chained buckets (ctor, insert, lookup, remove, clear,
  freeBuckets, freeNode, hash, iterateAll)
- **ByteArray**: Data container with cursor-based read

### 8b. Animation Resources (`.sai` format)

`AnimRes` (RTTI "7AnimRes" at 0x80adce88) parses `.sai` binary animation files:

- Compressed 3x4 bone matrices with 16-bit values and per-component sign flags (9 bits)
- Optional vertex animation tracks
- `AnimRes_sampleBoneMatrix` — interpolate bone matrix at time t
- `AnimRes_sampleVertexAnim` — interpolate vertex animation at time t
- `AnimNode` — animation tree nodes with parent resolution
- `AnimTrack` — tagged animation tracks (findByTag, init, deserialize)

### 8c. Audio Resources (`.sau` format)

`AudioData` (RTTI "9AudioData" at 0x80adced4, vtable 0x80adceb8):

- Custom ADPCM-like codec: 3-bit codes, 89-entry step size table, index delta table
- `AudioData_decompressADPCM` — decompresses to 16-bit PCM
- `SoundManager` — manages channels by priority, handles BGM looping/fade
- `SoundSource` — 3D positioned audio (position, direction, velocity, distance-based volume)

### 9. Audio subsystem (verified via decompilation)

#### AudioDevice structure (0x1ac bytes)

```c
struct AudioDevice {
    u8   master_volume;     // +0x00 (0x00-0xFF, initially 0xFF)

    // 16 mixing channels (stride = 4 bytes per array):
    s16* pcm_data[16];      // +0x04   PCM sample pointer (null = inactive)
    u32  sample_count[16];  // +0x44   total samples in buffer
    u32  play_pos[16];      // +0x84   current playback position
    u32  channel_vol[16];   // +0xC4   per-channel volume (0-7, default 7)
    u32  looping[16];       // +0x104  0=one-shot, 1=loop
    u32  muted[16];         // +0x144  1=muted/stopped, 0=active

    void* sem_mix;          // +0x184  OSSemCreate(1) — mixer lock
    void* sem_write;        // +0x188  OSSemCreate(1) — write sync
    s32*  mix_buffer;       // +0x190  0x640 bytes (400 s32 samples)
    s16*  out_buffer;       // +0x194  0x320 bytes (400 s16 samples)
    void* waveout_handle;   // +0x198  from waveout_open()
    u32   sample_rate;      // +0x19C  0x3E80 (16000 Hz)
    u16   bits_per_sample;  // +0x1A0  0x10 (16-bit)
    u8    channels_mono;    // +0x1A2  1 (mono)
    u8    initial_vol;      // +0x1A3  0x64 (100)
    u8    task_priority;    // +0x1A8  assigned by OSTaskCreate
};
```

#### Audio task flow (`AudioDevice_taskMain` at 0x80a47770)

```
AudioDevice_taskMain(AudioDevice* dev):
  OSSemPend(dev->sem_write, INFINITE)    // wait for go signal
  while (dev->channels_mono != 0):       // channels_mono used as alive flag
    OSSemPend(dev->sem_mix, INFINITE)    // lock mixer

    // zero mix_buffer (400 × s32)
    for ch in 0..15:
      if not muted[ch] and pcm_data[ch]:
        // mix: add samples scaled by channel_vol
        for i in 0..399:
          mix_buffer[i] += pcm_data[ch][play_pos] * channel_vol[ch]
          play_pos++
          if play_pos >= sample_count:
            if looping[ch]: play_pos = 0
            else: stop channel, break

    OSSemPost(dev->sem_mix)              // unlock mixer

    // apply master volume + clamp to s16
    for i in 0..399:
      sample = (mix_buffer[i] >> 8) * master_volume >> 8
      out_buffer[i] = clamp(sample, -32768, 32767)

    waveout_write(0, out_buffer)         // submit 800 bytes
    OSSemPost(dev->sem_write)            // signal done

  OSTaskDel(OS_PRIO_SELF)               // self-delete
```

Key audio functions (renamed in Binary Ninja):

| Address | Name | Description |
|---------|------|-------------|
| `0x80a47980` | `AudioDevice_init` | Create device, open waveout, spawn task |
| `0x80a47770` | `AudioDevice_taskMain` | OS task: 16-ch mixer → waveout_write loop |
| `0x80a47dbc` | `AudioDevice_playChannel` | Start PCM on channel (sem-protected) |
| `0x80a47e58` | `AudioDevice_stopChannel` | Stop single channel |
| `0x80a47ebc` | `AudioDevice_stopAll` | Stop all 16 channels |
| `0x80a47ba8` | `AudioDevice_destroy` | Close waveout, free buffers, delete sems |
| `0x80a4808c` | `AudioDevice_setVolume` | Set master volume (0-255, sem-protected) |
| `0x80a48000` | `AudioDevice_setChannelVol` | Set per-channel volume |
| `0x80a47f7c` | `AudioDevice_setLooping` | Set looping flag on channel |

#### OSTaskCreate usage

Task priority scanning: tries priority 0x10 (16) first, increments up to 0x3F (63):
```c
for (prio = 0x10; prio < 0x40; prio++) {
    if (OSTaskCreate(AudioDevice_taskMain, dev, stack, prio) == 0) {
        dev->task_priority = prio;
        break;
    }
}
```

### 10. Fixed-Point Math & Geometry

All 3D math uses 16.16 fixed-point (no FPU on MIPS32 target).

**Matrix operations** (35 functions at 0x80a639b4–0x80a7561c):
- `mat4_adjugate`, `mat4_transpose`, `mat4_inverse` (massive ~19KB function)
- `mat4_scale`, `mat4_translate`, `mat4_rotateX/Y/Z`, `mat4_rotateEuler`
- `mat4_fromQuaternion`, `mat4_makeAxisAngle`
- `mat4_lookAt/lookAtRH`, `mat4_frustum/frustumRH`
- `mat4_perspectiveFov/perspectiveFovRH`, `mat4_ortho/orthoRH`
- `mat4_viewport`, `mat4_shadow`, `mat4_makeReflection`
- Left-hand (LH) and right-hand (RH) variants for all projection matrices

**Geometry utilities**:
- `AABB_initInverse`, `AABB_expandByPoint`, `AABB_getCenter`
- `Plane_initFromRay`, `Plane_transformByMatrix`, `Plane_distanceToPoint`
- `Frustum_scalePlanes`, `Frustum_transformPlane{A,B,C}`, `Frustum_scaleMatrix`
- `fixedpoint_mul`, `fixedpoint_sqrt` (3 variants), `fixedpoint_atan`, `fixedpoint_acos`, `fixedpoint_atan2`
- `vec4_transformInverse`
- `Matrix4x4_multiply`

**Trig tables**: `getSinTable`, `lookupSin` — precomputed sin/cos lookup

**Software float** (no hardware FPU):
- `softfloat_mul/div/add/cmp/from_int/to_int/pack/unpack`
- `softdouble_add/sub/mul/from_int/to_int/negate/pack/unpack`

### 11. Networking (likely unused)

- `ClientSession`, `MyClientSession`, `TcpSession`,
  `HttpSession`, `SessionDevice`

## Game Structure

- **13 days** (not 7 as the title implies): `LoadDay 1` through `LoadDay 13`
- Each day loads scenes from `.\day%d\` directories
- Episodes within days: `.\day%d\ep0%d` (< 1000) or `.\day%d\ep%d` (>= 1000)
- Scenes are BSP maps with `.sbp` script files
- Day progression via `##LoadNextDay` → `GameEngine_loadNextDay`
- Day 5 has branching: `_1f_day5`, `1f_day5`, `2f_day5`, `3f_day5`

### Scene state machine (`Scene_update` at `0x80a3dffc`)

```
State 0: IDLE (waiting for next scene/load trigger)
  └── check save/load requests (slot%d.sav)

State 1: SCENE_ENTER (56 tick transition)
  ├── tick 8: load episode BSP, set camera, spawn entities
  ├── tick 31 (0x1f): play ambient audio, fade in
  └── tick 56 (0x38): → State 0

State 2: SCENE_RUNNING
  ├── billboard updates, camera follow
  ├── tick 24 (0x18): → State 3
  └── tick 28 (0x1c): show dialog, unfreeze camera

State 3: SCENE_TRANSITION
  ├── fade out, audio crossfade
  └── tick 36 (0x24): → State 0 (or 4)

State 4: SCENE_EXIT (36 tick cleanup)
  └── tick 36: → State 0, clear transition flags
```
- Save system: `slot%d.sav` (AES-128 encrypted, see below)
- Game state: `.\common\default.s3dsty`, `.\ui\state.sdt`
- Game over: `m_gameover`

## File Formats

| Extension | Purpose |
|-----------|---------|
| `.sbp` | SimpleScript bytecode (script packs) |
| `.sbn` | Font/string binary data |
| `.sdt` | State data tables |
| `.sst` | Script scene trees (typed node archive) |
| `.s3dsty` | s3d engine style/config |
| `.sav` | Save game slots (AES-128 encrypted) |
| BSP (custom) | 3D level: geometry, lightmaps, PVS, entities, nav mesh |

### Save File Format (`slot%d.sav`)

Save files are AES-128-CBC encrypted (`GameEngine_saveSaveFile` at `0x80a16768`):

```
1. AES key: 16 bytes hardcoded at stack frame
   [0xa6, 0xf2, 0x28, 0x04, 0x6d, 0xbd, 0x2f, 0x95,
    0xc4, 0x6b, 0xc0, 0xa8, 0x7b, 0x6d, 0xfb, 0xd1]

2. File layout:
   - 0x80 bytes: game state block (encrypted separately)
   - MD5 hash of decrypted state → encrypted → appended (0x10 bytes)
   - 0x80 bytes: secondary state (per-slot data, encrypted)
   - MD5 hash → encrypted → appended (0x10 bytes)
   - 0x18 bytes: player save data → encrypted → appended

3. Signature check: decrypted state validated against "DingooGames2006 7days"
```

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
│   ├── Creature (state machine: Stand/Walk/Idle/Attack/Hurt/Death/Chase)
│   │   └── Enemy (decideAction)
│   │       ├── Bully / BullyDay3 / BullyDay5
│   │       ├── Ghost / DarkKen
│   │       ├── Lamper / Worm / Weed
│   │       └── ElectricSaw
│   ├── Lord / Victor / Kate / Sam / Prisoner
│   ├── FatMaid / Maid / Maid2 / Maid3
│   ├── Sword / GunFire
│   ├── BullyWeapon
│   ├── Pickable / Pushable / Breakable
│   └── Trap / StayBox
├── Scene
├── Bsp
├── Animation / AnimRes / AnimNode / AnimTrack
├── AudioData
├── Texture / VID_impl
├── Font
├── Camera / CameraSpot
├── Spline / SplineCurve
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

Stream (0x80ad90d4)
├── InputStream (0x80ad90b8)
│   ├── InputFileStream (0x80ad9090)
│   └── InputDataStream (0x80ad90a4) — big-endian deserialization
└── OutputStream
    ├── OutputFileStream
    └── OutputDataStream — big-endian serialization

ByteArray — cursor-based data read
DynArray — growable array (4/28/52-byte element variants)
DynBuffer/ResizableArray — growable buffer with serialization
HashMap — hash map with chained buckets

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

Effects: BloodEffect, Particle

C++ Runtime:
├── type_info / __class_type_info / __si_class_type_info / __vmi_class_type_info
├── __dynamic_cast, __cxa_throw, __cxa_begin_catch, __cxa_end_catch
├── __gxx_personality_v0 (GCC exception personality)
├── _Unwind_RaiseException, _Unwind_Resume, _Unwind_Backtrace
├── DWARF FDE/CIE parsing and CFI execution
└── Software float/double (no FPU): mul, div, add, cmp, pack/unpack, int↔float
```

## Input System (verified via decompilation)

### Trampoline wrappers (0x80a00000 region)

```c
lcd_get_frame_wrapper(a1, a2) → _lcd_get_frame(a1, a2)    // 0x80a00000
lcd_set_frame_wrapper(a1, a2) → _lcd_set_frame(a1, a2)    // 0x80a0001c
kbd_get_key_wrapper()         → printf("kbd_get_key\n")    // 0x80a00038
                                 _kbd_get_key()             //   (debug print!)
kbd_get_status_wrapper(a1,a2) → _kbd_get_status(a1, a2)    // 0x80a00060
sys_judge_event_wrapper(a1,a2)→ _sys_judge_event(a1, a2)   // 0x80a0007c
```

### _kbd_get_status return structure

`_kbd_get_status` writes to a 12-byte struct at `0x80b3a300`:

```c
struct KbdStatus {
    u32  raw_x;      // +0x00 (0x80b3a300) — analog/axis X
    u32  raw_y;      // +0x04 (0x80b3a304) — analog/axis Y
    u32  buttons;    // +0x08 (0x80b3a308) — button bitmask
};
// Previous frame stored at 0x80b3a30c (prev_x, prev_y, prev_buttons)
```

### input_dispatch detail (0x80a0046c)

Calls `kbd_get_status_wrapper(&kbd_status)`, then for each button bit:
- If bit SET in current `buttons` → call `Engine_keyDown(engine->input_mgr, action_id, 1)`
- If bit SET in `prev_buttons` (released) → call `Engine_keyUp(engine->input_mgr, action_id, 1)`

`input_mgr` is at `engine + 0x8c`.

### Engine_keyDown (0x80a114fc) — key repeat logic

```c
Engine_keyDown(input_mgr, action_id, force):
  if (action_id == 0) return
  slot = &input_mgr[action_id]  // each slot is a struct
  // guard: if force && input blocked && action 0x11-0x13: skip
  if (slot->state & 1):  // already pressed
    slot->repeat_count++
    if (repeat_count >= 6):
      slot->state |= 0x04    // mark as "held/repeated"
      slot->repeat_count = 0
  else:
    slot->state = 3           // newly pressed (bit0=down, bit1=just_pressed)
```

### Engine_keyUp (0x80a115c4) — release

```c
Engine_keyUp(input_mgr, action_id, force):
  if (action_id == 0) return
  slot = &input_mgr[action_id]
  if (slot->state & 1):      // was pressed
    slot->state = 8           // released flag
    slot->repeat_count = 0
```

### Input slot structure

```c
struct InputSlot {      // per action_id, within input_mgr array
    u8  state;          // +0x01: bit0=down, bit1=just_pressed, bit2=held, bit3=released
    u8  repeat_count;   // +0x19: frames held (triggers "held" at 6)
};
// Indexed by action_id (0x02..0x12), stride determined by input_mgr layout
```

## uC/OS-II API Usage Summary

### API calls and their callers

| API | Callers | Purpose |
|-----|---------|---------|
| `OSTaskCreate` | `AudioDevice_init` ×2 variants | Create audio mixer task |
| `OSTaskDel(0xFF)` | `AudioDevice_taskMain` | Self-delete (OS_PRIO_SELF=0xFF) |
| `OSSemCreate(1)` | `AudioDevice_init` ×4 | Binary semaphores for mixer sync |
| `OSSemPend(sem,∞)` | `AudioDevice_taskMain`, `_playChannel`, `_stopChannel`, etc. | Lock mixer/write access |
| `OSSemPost(sem)` | Same functions | Unlock |
| `OSSemDel(sem,1)` | `AudioDevice_destroy` ×2 | Delete semaphores (opt=1: always delete) |
| `OSTimeDly(ticks)` | `s3dTimer_sleep` | Sleep for microseconds/10000 ticks |
| `OSTimeGet()` | `s3dTimer_getTimeMicros` | Get time × 10000 = microseconds |
| `OSCPUSaveSR` | Not called directly | (trampoline exists but unused) |
| `OSCPURestoreSR` | Not called directly | (trampoline exists but unused) |
| `GetTickCount` | Not called by app code | (chains to `_sys_judge_event` via trampoline) |

### Key observations for HLE implementation

1. **Single OS task**: Only the audio mixer runs as a separate task. The game
   itself runs single-threaded in the main `AppMain` → `s3d_main` flow.

2. **Semaphores as mutexes**: All `OSSemCreate(1)` calls create binary
   semaphores used as mutexes (initial count=1). No counting semaphores.

3. **Task priority scanning**: Audio task probes priorities 0x10–0x3F until
   `OSTaskCreate` succeeds. HLE can assign any priority.

4. **Timer granularity**: `OSTimeGet()` returns OS ticks. The engine multiplies
   by 10000 to get microseconds, implying 1 tick = 10ms (100 Hz OS tick rate).
   `OSTimeDly` divides microseconds by 10000 to get ticks.

5. **No direct ISR usage**: `OSCPUSaveSR`/`OSCPURestoreSR` are imported but
   never called from app code. Cache ops (`__icache_invalidate_all`,
   `__dcache_writeback_all`) are stub no-ops at `0x80a00154`–`0x80a0016c`.

6. **`_sys_judge_event`**: Called every frame in main loop. Returns < 0 to
   signal app should exit (e.g., user pressed home button via `vxGoHome`).

### Trampoline table layout (0x80a001d0 – 0x80a00408)

All CCDL imports are at fixed 8-byte intervals. Each is a `NOP; JR $RA` stub
that gets patched by the OS loader. In the ELF they appear as named functions.

```
0x80a001d0  abort          0x80a001d8  printf
0x80a001e0  sprintf        0x80a001e8  fprintf
0x80a001f0  strncasecmp    0x80a001f8  malloc
0x80a00200  realloc        0x80a00208  free
0x80a00210  fread          0x80a00218  fwrite
0x80a00220  fseek          0x80a00228  LcdGetDisMode
0x80a00230  vxGoHome       0x80a00238  StartSwTimer
0x80a00240  free_irq       0x80a00248  fsys_RefreshCache
0x80a00250  strlen         0x80a00258  _lcd_set_frame
0x80a00260  _lcd_get_frame 0x80a00268  lcd_get_cframe
0x80a00270  ap_lcd_set_frame 0x80a00278  lcd_flip
0x80a00280  __icache_invalidate_all  0x80a00288  __dcache_writeback_all
0x80a00290  TaskMediaFunStop  0x80a00298  OSCPUSaveSR
0x80a002a0  OSCPURestoreSR 0x80a002a8  serial_getc
0x80a002b0  serial_putc    0x80a002b8  _kbd_get_status
0x80a002c0  get_game_vol   0x80a002c8  _kbd_get_key
0x80a002d0  fsys_fopen     0x80a002d8  fsys_fread
0x80a002e0  fsys_fclose    0x80a002e8  fsys_fseek
0x80a002f0  fsys_ftell     0x80a002f8  fsys_remove
0x80a00300  fsys_rename    0x80a00308  fsys_ferror
0x80a00310  fsys_feof      0x80a00318  fsys_fwrite
0x80a00320  fsys_findfirst 0x80a00328  fsys_findnext
0x80a00330  fsys_findclose 0x80a00338  fsys_flush_cache
0x80a00340  USB_Connect    0x80a00348  udc_attached
0x80a00350  USB_No_Connect 0x80a00358  waveout_open
0x80a00360  waveout_close  0x80a00368  waveout_close_at_once
0x80a00370  waveout_set_volume  0x80a00378  HP_Mute_sw
0x80a00380  waveout_can_write   0x80a00388  waveout_write
0x80a00390  pcm_can_write  0x80a00398  pcm_ioctl
0x80a003a0  OSTimeGet      0x80a003a8  OSTimeDly
0x80a003b0  OSSemPend      0x80a003b8  OSSemPost
0x80a003c0  OSSemCreate    0x80a003c8  OSTaskCreate
0x80a003d0  OSSemDel       0x80a003d8  OSTaskDel
0x80a003e0  GetTickCount   0x80a003e8  _sys_judge_event
0x80a003f0  fsys_fopenW    0x80a003f8  __to_unicode_le
0x80a00400  __to_locale_ansi  0x80a00408  get_current_language
```
