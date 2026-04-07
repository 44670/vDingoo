/// Audio system: IMA ADPCM decoder + SDL2 audio playback.
///
/// SAU format: raw 4-bit IMA ADPCM, no header.
/// Audio specs: 16kHz mono 16-bit signed PCM.

use crate::fs_app::AppFs;
use sdl2::audio::{AudioCallback, AudioDevice, AudioSpecDesired};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// IMA ADPCM step size table (89 entries).
const STEP_TABLE: [i32; 89] = [
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60,
    66, 73, 80, 88, 97, 107, 118, 130, 143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371,
    408, 449, 494, 544, 598, 658, 724, 796, 876, 963, 1060, 1166, 1282, 1411, 1552, 1707, 1878,
    2066, 2272, 2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358, 5894, 6484, 7132, 7845,
    8630, 9493, 10442, 11487, 12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623, 27086,
    29794, 32767,
];

/// IMA ADPCM index adjustment table.
const INDEX_TABLE: [i32; 8] = [-1, -1, -1, -1, 2, 4, 6, 8];

/// Decode IMA ADPCM data to 16-bit PCM.
pub fn decode_adpcm(data: &[u8]) -> Vec<i16> {
    let mut output = Vec::with_capacity(data.len() * 2);
    let mut predictor: i32 = 0;
    let mut step_index: i32 = 0;

    for &byte in data {
        for nibble_idx in 0..2 {
            let nibble = if nibble_idx == 0 {
                (byte & 0x0F) as i32
            } else {
                ((byte >> 4) & 0x0F) as i32
            };

            let step = STEP_TABLE[step_index as usize];
            let mut diff = ((nibble & 7) * step) >> 2;
            diff += step >> 3;

            step_index += INDEX_TABLE[(nibble & 7) as usize];
            step_index = step_index.clamp(0, 88);

            if nibble & 8 != 0 {
                diff = -diff;
            }
            predictor += diff;
            predictor = predictor.clamp(-32768, 32767);

            output.push(predictor as i16);
        }
    }
    output
}

/// A loaded audio clip.
pub struct AudioClip {
    pub samples: Vec<i16>, // 16-bit mono PCM at 16kHz
}

/// Playback state for a single channel.
struct ChannelState {
    clip_name: String,
    position: usize,
    volume: f32, // 0.0 - 1.0
    looping: bool,
    playing: bool,
}

const MAX_CHANNELS: usize = 16;
const SAMPLE_RATE: i32 = 16000;

/// Shared state between audio thread and main thread.
struct MixerState {
    channels: Vec<Option<ChannelState>>,
    clips: HashMap<String, Arc<AudioClip>>,
    master_volume: f32,
}

struct AudioMixer {
    state: Arc<Mutex<MixerState>>,
}

impl AudioCallback for AudioMixer {
    type Channel = i16;

    fn callback(&mut self, out: &mut [i16]) {
        let mut state = match self.state.lock() {
            Ok(s) => s,
            Err(_) => {
                for sample in out.iter_mut() {
                    *sample = 0;
                }
                return;
            }
        };

        let MixerState { ref clips, ref mut channels, master_volume } = *state;

        for sample in out.iter_mut() {
            let mut mix: i32 = 0;

            for ch in channels.iter_mut() {
                if let Some(ref mut channel) = ch {
                    if !channel.playing {
                        continue;
                    }
                    if let Some(clip) = clips.get(&channel.clip_name) {
                        if channel.position < clip.samples.len() {
                            let s = clip.samples[channel.position] as f32 * channel.volume;
                            mix += s as i32;
                            channel.position += 1;
                        } else if channel.looping {
                            channel.position = 0;
                        } else {
                            channel.playing = false;
                        }
                    }
                }
            }

            mix = (mix as f32 * master_volume) as i32;
            *sample = mix.clamp(-32768, 32767) as i16;
        }
    }
}

/// Audio system manager.
pub struct AudioSystem {
    _device: AudioDevice<AudioMixer>,
    state: Arc<Mutex<MixerState>>,
}

impl AudioSystem {
    pub fn new(sdl_audio: &sdl2::AudioSubsystem) -> Self {
        let mixer_state = Arc::new(Mutex::new(MixerState {
            channels: (0..MAX_CHANNELS).map(|_| None).collect(),
            clips: HashMap::new(),
            master_volume: 0.8,
        }));

        let desired_spec = AudioSpecDesired {
            freq: Some(SAMPLE_RATE),
            channels: Some(1),
            samples: Some(512),
        };

        let state_clone = mixer_state.clone();
        let device = sdl_audio
            .open_playback(None, &desired_spec, |_spec| AudioMixer {
                state: state_clone,
            })
            .expect("Failed to open audio device");

        device.resume();

        AudioSystem {
            _device: device,
            state: mixer_state,
        }
    }

    /// Load a SAU file (raw IMA ADPCM) and cache it.
    pub fn load_clip(&self, name: &str, fs: &AppFs) -> bool {
        {
            let state = self.state.lock().unwrap();
            if state.clips.contains_key(name) {
                return true;
            }
        }

        let path = format!(".\\common\\{}", name);
        let data = match fs.read(&path) {
            Some(d) => d,
            None => return false,
        };

        let samples = decode_adpcm(data);
        if samples.is_empty() {
            return false;
        }

        println!("Audio: loaded {name} ({} samples, {:.1}s)", samples.len(), samples.len() as f32 / SAMPLE_RATE as f32);

        let clip = Arc::new(AudioClip { samples });
        let mut state = self.state.lock().unwrap();
        state.clips.insert(name.to_string(), clip);
        true
    }

    /// Play a clip on the first available channel.
    pub fn play(&self, name: &str, looping: bool, volume: f32) -> Option<usize> {
        let mut state = self.state.lock().unwrap();

        // Find a free channel
        let ch_idx = state.channels.iter().position(|ch| {
            ch.as_ref().map_or(true, |c| !c.playing)
        })?;

        state.channels[ch_idx] = Some(ChannelState {
            clip_name: name.to_string(),
            position: 0,
            volume: volume.clamp(0.0, 1.0),
            looping,
            playing: true,
        });

        Some(ch_idx)
    }

    /// Stop all channels.
    pub fn stop_all(&self) {
        let mut state = self.state.lock().unwrap();
        for ch in &mut state.channels {
            *ch = None;
        }
    }

    /// Stop a specific channel.
    #[allow(dead_code)]
    pub fn stop_channel(&self, idx: usize) {
        let mut state = self.state.lock().unwrap();
        if idx < state.channels.len() {
            state.channels[idx] = None;
        }
    }
}
