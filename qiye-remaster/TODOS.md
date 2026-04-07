# 七夜 Remaster — TODO

## Current State: Textured BSP Scene Viewer

What works today:
- Free-cam WASD+mouse fly-through of any BSP map (PageDown/PageUp to cycle 152 maps)
- BSP rendering with diffuse textures + lightmap atlas
- SOJ entity models rendered at BSP positions with single-bone animation loops
- Debug wireframe overlay for entities and trigger volumes (F key)
- Manual script stepper (Enter = step one SST command, Tab = dump all)
- Audio playback works IF you manually step to a PlayBgm/PlaySfx script command
- HUD text: map name, day, player HP/state, enemy count, health bar
- Player state machine updates in memory (Backspace toggles player mode)

What does NOT work:
- Player is invisible (model never rendered)
- Enemies are invisible (no render call)
- Player floats at Y=0 regardless of BSP geometry (no collision)
- Scripts never auto-run — only manual Enter stepping
- Triggers detected but do nothing (just println to stdout)
- No scene transitions, no story progression, no game flow
- Dialog text is ASCII only — Chinese characters don't render
- Audio never plays on its own — requires manual script stepping

---

## Phase 1 — Make it a game (not a viewer)

### P0: Core loop (must have)

- [ ] **Render player model** — `player.model_name` is set from BSP but never drawn. Add render call in game loop at `player.pos` with facing rotation matrix.
- [ ] **Render enemies** — Enemy structs have positions and model names but are never rendered. Draw them.
- [ ] **BSP collision** — Player/enemies clamp to Y=0. Need `trace_box()` using BSP planes/nodes for ground detection, wall sliding, gravity.
- [ ] **Auto-run scripts** — `auto_run` is hardcoded `false`. Scripts must execute automatically on scene load with wait/yield support (WaitFrames already implemented, just needs auto stepping).
- [ ] **Wire triggers → actions** — `TriggerEvent` enter/exit events are detected but only logged. Connect: DoorTrigger→scene switch, TalkTrigger→dialog/script, EventTrigger→script, CameraTrigger→camera mode.
- [ ] **Scene transitions** — No way to advance the game. Need script command or trigger to load next BSP + scripts (day/episode flow).
- [ ] **Chinese font (SBN)** — All dialog is Chinese. Parse `.sbn` font files from PAK, render CJK glyphs in dialog box.

### P1: Gameplay systems

- [ ] **Implement script commands** — Only 12 of ~80 dispatched. Priority: camera (11-25), entity control (106-128), creature/enemy (74-93), dialog variants (31-38), fade (216), trigger control (130-136).
- [ ] **Skeletal animation** — Only bone 0 used as rigid-body transform. Need per-vertex bone skinning using `SojVertex.bone_index`.
- [ ] **Animation state switching** — One animation loops forever per model. Player needs Stand/Run/Attack/Hurt/Die animations. Load multiple `.sai` per model, switch on `anim_state_id`.
- [ ] **Camera script control** — `SetupCamera`, `SetCameraFollow`, `AttachCameraToPlayer` parsed but never dispatched. Cutscenes need scripted camera.
- [ ] **Choice dialogs** — `ShowDialogWithChoice` / `GetDialogResult` — required for branching story.
- [ ] **Enemy type mapping** — All enemies spawn as Generic. Map BSP model names → types (Bully, Ghost, DarkKen etc.) with correct stats from RE.

## Phase 2 — Polish

- [ ] **Save/Load** — `GameData` serialization works, never called. Need save slots + key binding.
- [ ] **BGM/SFX separation** — `StopBgm` kills all sounds. Dedicate channel 0 for BGM.
- [ ] **3D audio** — `PlaySfx3D` plays flat. Add distance attenuation + stereo panning.
- [ ] **Camera collision** — Follow cam clips through walls. BSP trace from target to camera.
- [ ] **PVS / frustum culling** — All faces rendered every frame. BSP leaf + PVS culling.
- [ ] **Alpha blending** — SOJ `render_mode` parsed but ignored. Support alpha blend / additive.
- [ ] **Scroll UV** — SOJ materials have `scroll_u`/`scroll_v`, ignored.
- [ ] **Lightmap styles** — Only style 0 used. Animated lights need styles 1-3.
- [ ] **Fog** — Original has fog, not implemented.
- [ ] **Invincibility frames** — Player takes damage every frame in attack window.
- [ ] **Menu system** — No pause, inventory, or map screen.
- [ ] **Loading screen / fade** — `FadeScreen` / `ShowLoadingScreen` not implemented.
- [ ] **Animation interpolation** — Naive matrix lerp is geometrically wrong. Original uses lookupSin easing.
- [ ] **F key conflict** — Attack and debug toggle share same key.
