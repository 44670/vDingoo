use sdl2::keyboard::Scancode;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Action {
    MoveForward,
    MoveBack,
    MoveLeft,
    MoveRight,
    MoveUp,
    MoveDown,
    Sprint,
    Attack,
    Confirm,
    Cancel,
    MenuToggle,
    NextMap,
    PrevMap,
}

const ACTION_COUNT: usize = 13;
const REPEAT_THRESHOLD: u8 = 6;

#[derive(Clone, Copy, Default)]
struct ButtonState {
    down: bool,
    just_pressed: bool,
    just_released: bool,
    hold_frames: u8,
}

pub struct InputState {
    buttons: [ButtonState; ACTION_COUNT],
    pub mouse_dx: f32,
    pub mouse_dy: f32,
    pub quit: bool,
}

impl InputState {
    pub fn new() -> Self {
        Self {
            buttons: [ButtonState::default(); ACTION_COUNT],
            mouse_dx: 0.0,
            mouse_dy: 0.0,
            quit: false,
        }
    }

    pub fn begin_frame(&mut self) {
        for btn in &mut self.buttons {
            btn.just_pressed = false;
            btn.just_released = false;
        }
        self.mouse_dx = 0.0;
        self.mouse_dy = 0.0;
    }

    pub fn handle_event(&mut self, event: &sdl2::event::Event) {
        use sdl2::event::Event;
        match event {
            Event::Quit { .. } => self.quit = true,
            Event::KeyDown {
                scancode: Some(Scancode::Escape),
                ..
            } => self.quit = true,
            Event::MouseMotion { xrel, yrel, .. } => {
                self.mouse_dx += *xrel as f32;
                self.mouse_dy += *yrel as f32;
            }
            _ => {}
        }
    }

    pub fn update_from_keyboard(&mut self, keys: &sdl2::keyboard::KeyboardState) {
        self.set_action(Action::MoveForward, keys.is_scancode_pressed(Scancode::W));
        self.set_action(Action::MoveBack, keys.is_scancode_pressed(Scancode::S));
        self.set_action(Action::MoveLeft, keys.is_scancode_pressed(Scancode::A));
        self.set_action(Action::MoveRight, keys.is_scancode_pressed(Scancode::D));
        self.set_action(Action::MoveUp, keys.is_scancode_pressed(Scancode::Space));
        self.set_action(Action::MoveDown, keys.is_scancode_pressed(Scancode::LShift));
        self.set_action(Action::Sprint, keys.is_scancode_pressed(Scancode::LCtrl));
        self.set_action(Action::Attack, keys.is_scancode_pressed(Scancode::F));
        self.set_action(Action::Confirm, keys.is_scancode_pressed(Scancode::Return));
        self.set_action(Action::Cancel, keys.is_scancode_pressed(Scancode::Backspace));
        self.set_action(Action::MenuToggle, keys.is_scancode_pressed(Scancode::Tab));
        self.set_action(Action::NextMap, keys.is_scancode_pressed(Scancode::PageDown));
        self.set_action(Action::PrevMap, keys.is_scancode_pressed(Scancode::PageUp));
    }

    fn set_action(&mut self, action: Action, pressed: bool) {
        let btn = &mut self.buttons[action as usize];
        if pressed && !btn.down {
            btn.just_pressed = true;
            btn.hold_frames = 0;
        } else if !pressed && btn.down {
            btn.just_released = true;
            btn.hold_frames = 0;
        } else if pressed {
            btn.hold_frames = btn.hold_frames.saturating_add(1);
        }
        btn.down = pressed;
    }

    pub fn is_down(&self, action: Action) -> bool {
        self.buttons[action as usize].down
    }

    pub fn just_pressed(&self, action: Action) -> bool {
        self.buttons[action as usize].just_pressed
    }

    #[allow(dead_code)]
    pub fn just_released(&self, action: Action) -> bool {
        self.buttons[action as usize].just_released
    }

    #[allow(dead_code)]
    pub fn is_repeating(&self, action: Action) -> bool {
        let btn = &self.buttons[action as usize];
        btn.just_pressed || (btn.down && btn.hold_frames >= REPEAT_THRESHOLD)
    }
}
