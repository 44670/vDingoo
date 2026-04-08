/// SST script engine — parses and executes game scripts.
///
/// SST binary format:
///   Commands separated by delimiter: 24 00 00 00 00 00
///   Each command: u16 cmd_id, u16 extra, u16 arg_count
///   Each arg: u16 type, u16 size, [size bytes data]
///   Arg types: 2=int(4), 3=string(var), 4=raw_int(4), 5=fixed(4)

#[derive(Debug, Clone)]
pub enum ArgValue {
    Int(i32),
    Fixed(f32),
    Str(String),
    Raw(i32),
}

impl std::fmt::Display for ArgValue {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            ArgValue::Int(v) => write!(f, "{v}"),
            ArgValue::Fixed(v) => write!(f, "{v:.2}"),
            ArgValue::Str(s) => write!(f, "\"{s}\""),
            ArgValue::Raw(v) => write!(f, "0x{v:x}"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScriptCommand {
    pub id: u16,
    pub args: Vec<ArgValue>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScriptState {
    Running,
    WaitFrames(u32),
    Done,
}

pub struct ScriptEngine {
    commands: Vec<ScriptCommand>,
    pc: usize,
    pub state: ScriptState,
    pub auto_run: bool,
}

fn read_u16_le(d: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([d[o], d[o + 1]])
}
fn read_i32_le(d: &[u8], o: usize) -> i32 {
    i32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]])
}

impl ScriptEngine {
    pub fn parse_sst(data: &[u8]) -> Self {
        let mut commands = Vec::new();
        let mut off = 0;

        while off + 12 <= data.len() {
            // Scan for dispatch node marker: 24 00 00 00 00 00
            // 0x24 = AnimNode type "dispatch to handler", followed by 00 00 (flags) 00 00 (padding)
            if data[off] != 0x24 || data[off + 1..off + 6] != [0, 0, 0, 0, 0] {
                off += 2;
                continue;
            }

            let cmd_id = read_u16_le(data, off + 6);
            let extra = read_u16_le(data, off + 8);
            let arg_count = read_u16_le(data, off + 10);

            // Sanity check — skip nodes with extra != 0 (sub-tree refs, not flat args)
            if cmd_id >= 300 || arg_count >= 20 || extra != 0 {
                off += 2;
                continue;
            }

            let mut aoff = off + 12;
            let mut args = Vec::with_capacity(arg_count as usize);
            let mut valid = true;

            for _ in 0..arg_count {
                if aoff + 4 > data.len() {
                    valid = false;
                    break;
                }
                let atype = read_u16_le(data, aoff);
                let asize = read_u16_le(data, aoff + 2) as usize;

                if aoff + 4 + asize > data.len() {
                    valid = false;
                    break;
                }

                match atype {
                    5 if asize == 4 => {
                        let v = read_i32_le(data, aoff + 4);
                        args.push(ArgValue::Fixed(v as f32 / 65536.0));
                        aoff += 8;
                    }
                    2 if asize == 4 => {
                        args.push(ArgValue::Int(read_i32_le(data, aoff + 4)));
                        aoff += 8;
                    }
                    3 => {
                        let s = &data[aoff + 4..aoff + 4 + asize];
                        let nul = s.iter().position(|&b| b == 0).unwrap_or(asize);
                        args.push(ArgValue::Str(
                            String::from_utf8_lossy(&s[..nul]).into_owned(),
                        ));
                        aoff += 4 + asize;
                    }
                    4 if asize == 4 => {
                        args.push(ArgValue::Raw(read_i32_le(data, aoff + 4)));
                        aoff += 8;
                    }
                    _ => {
                        // Unknown arg type — mark invalid and stop
                        valid = false;
                        break;
                    }
                }
            }

            if valid {
                commands.push(ScriptCommand { id: cmd_id, args });
            }
            off += 2;
        }

        ScriptEngine {
            commands,
            pc: 0,
            state: ScriptState::Running,
            auto_run: false,
        }
    }

    pub fn command_count(&self) -> usize {
        self.commands.len()
    }

    pub fn step(&mut self) -> Option<&ScriptCommand> {
        if self.pc >= self.commands.len() {
            self.state = ScriptState::Done;
            return None;
        }

        let cmd = &self.commands[self.pc];
        self.pc += 1;
        Some(cmd)
    }

    /// Set script to wait N frames before resuming.
    pub fn wait_frames(&mut self, n: u32) {
        self.state = ScriptState::WaitFrames(n);
    }

    pub fn update(&mut self) {
        match self.state {
            ScriptState::WaitFrames(n) if n > 1 => {
                self.state = ScriptState::WaitFrames(n - 1);
            }
            ScriptState::WaitFrames(_) => {
                self.state = ScriptState::Running;
            }
            _ => {}
        }
    }

    pub fn is_done(&self) -> bool {
        self.state == ScriptState::Done
    }

    pub fn reset(&mut self) {
        self.pc = 0;
        self.state = ScriptState::Running;
    }
}

/// Get human-readable command name from SST command ID.
///
/// Command IDs map to ScriptTable entries: cmd_id = (table_slot - 0x1a) / 2
/// IDs 0-10 are in the AnimScriptHandler's own table (not tree flow control).
/// Tree control flow (SetVar/Compare/Jump) is in AnimNode structure, not command IDs.
pub fn command_name(id: u16) -> &'static str {
    match id {
        // AnimScriptHandler built-in commands (0-10)
        0 => "CheckButtonJustPressed0",
        1 => "CheckButtonJustPressed1",
        2 => "CheckDpadHeld",
        3 => "CheckButtonHeld",
        4 => "CheckButtonPressed",
        5 => "GetAnalogX",
        6 => "GetAnalogY",
        7 => "LogMessage",
        8 => "SetFogColorTop",
        9 => "SetFogColorBottom",
        10 => "LoadNextDay",
        11 => "SetupCamera",
        12 => "ResetCamera",
        13 => "RestoreCamera",
        14 => "CheckBossDefeated",
        15 => "SetCameraFlag",
        16 => "GetCameraFlag",
        17 => "ResetMenu",
        18 => "OpenMenu",
        19 => "SetPauseFlag",
        20 => "RestoreCameraAndSnap",
        21 => "SetCameraLookAt",
        22 => "SetCameraFollow",
        23 => "CreateEmitter",
        24 => "GetCameraSpeed",
        25 => "AttachCameraToPlayer",
        26 => "Nop",
        27 => "SpawnEntity",
        28 => "SetCameraShakeParams",
        29 => "SetCameraShakeAmplitude",
        30 => "ShowDialog",
        31 => "SetDialogParam",
        32 => "SetDialogParamEx",
        33 => "ShowAutoDialog",
        34 => "ShowDialogWithChoice",
        35 => "GetDialogResult",
        36 => "CheckDialogDone",
        37 => "IsDialogActive",
        38 => "ShowLoadingScreen",
        41 => "PlayBgm",
        42 => "StopBgm",
        43 => "AddMusic",
        44 => "SetMusicVolume",
        45 => "PlaySfx",
        46 => "StopAllSounds",
        47 => "PlaySfx3D",
        48 => "FreeAllSounds",
        49 => "SetSceneFlag",
        50 => "SetGameDataFlag",
        51 => "GetGameDataFlag",
        52 => "ClearGameDataFlag",
        53 => "ShowStandardMsg",
        55 => "SetBspLightEnabled",
        57 => "SetRenderMode",
        58 => "SetDialogOverlay",
        59 => "SetLightColor",
        60 => "SetDepthBuffer",
        61 => "SetMenuLayout",
        62 => "SetMenuMode",
        63 => "SetMenuField",
        64 => "GetPlayerPosX",
        65 => "GetPlayerPosY",
        66 => "GetPlayerPosZ",
        67 => "GetPlayerRotY",
        68 => "CountActiveEntities",
        69 => "CountCollectedItems",
        70 => "SetCameraShake",
        71 => "SetPlayerInvincible",
        73 => "PausePlayer",
        74 => "SetCreatureFaceTarget",
        75 => "SetPlayerClip",
        76 => "SetCreatureAnimation",
        77 => "IsCreatureAnimDone",
        78 => "SetPlayerPosition",
        79 => "MoveCreatureToPos",
        80 => "CheckCreatureNavTarget",
        85 => "SetCreatureVisible",
        86 => "IsCreatureActive",
        87 => "GetCreatureHP",
        88 => "SetCreatureHP",
        90 => "SetCreatureState",
        91 => "SetCreatureAction",
        92 => "GetCreatureCount",
        93 => "CheckAllEnemiesDead",
        94 => "IsLoadingComplete",
        95 => "SpawnParticle",
        96 => "GetDay",
        97 => "GetQTEResult",
        99 => "ResetSceneState",
        100 => "IsSceneReady",
        101 => "SetSceneVar",
        103 => "SetPlayerModel",
        104 => "IsPlayerAlive",
        105 => "ResetScene",
        106 => "SpawnEntityEx",
        107 => "SetEntityAngle",
        108 => "SetEntitySpeed",
        110 => "SpawnEntityWithParams",
        111 => "SetEntityVisible",
        112 => "PlayAnimation",
        113 => "SetEntityMoveSpeed",
        115 => "SetTalkTriggerActive",
        116 => "SetEntityAreaParams",
        117 => "SetEntityState",
        118 => "SetEntityFieldByIndex",
        119 => "GetEntityAnimState",
        120 => "ResetEntityTarget",
        121 => "SetEntityFullParams",
        122 => "SetEntityAIEnabled",
        123 => "SetEntityTarget",
        124 => "SetEntityBounds",
        125 => "SetEntityCollision",
        126 => "SetEntityPath",
        127 => "SetEntityAnimation",
        128 => "SetEntityAnimLoop",
        130 => "SetTriggerPosition",
        131 => "AddTriggerCameraNode",
        132 => "AddTriggerWaypoint",
        133 => "HasTriggerData",
        134 => "IsTriggerActive",
        135 => "EnableTrigger",
        136 => "RemoveEntity",
        137 => "SpawnEnemy",
        138 => "SpawnProjectile",
        139 => "SetEnemyInvincible",
        140 => "SetEntityAI",
        141 => "IsEnemyAlive",
        142 => "IsEnemyDead",
        143 => "GetEnemyHP",
        144 => "SetEnemyHP",
        147 => "SetEnemyFaceTarget",
        148 => "SetEnemyPosition",
        152 => "SetEnemyAggressive",
        153 => "KillEnemy",
        154 => "RemoveEnemy",
        155 => "RemoveEnemiesByType",
        156 => "RemoveAllEnemies",
        161 => "SetStayBoxBounds",
        162 => "AddStayBoxExit",
        163 => "AddStayBoxEntry",
        164 => "LinkEnemyToStayBox",
        165 => "SetEnemyKnockback",
        166 => "SetEnemyAnimations",
        175 => "RemovePickTrigger",
        176 => "SpawnPickTrigger",
        177 => "RemoveActTrigger",
        178 => "SpawnActTrigger",
        179 => "SetActTriggerTarget",
        180 => "SetActTriggerAnim",
        182 => "SetActTriggerSfx",
        185 => "ConfigureActTrigger",
        186 => "SetActTriggerPhysics",
        191 => "LoadActTriggerModels",
        194 => "SetActTriggerScale",
        198 => "SetActTriggerPosition",
        199 => "MoveActTrigger",
        200 => "CheckActTriggerProximity",
        212 => "CollectItem",
        213 => "CheckItemCollected",
        214 => "UseItem",
        216 => "FadeScreen",
        _ => "Unknown",
    }
}
