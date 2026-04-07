mod animation;
mod audio;
mod bsp;
mod camera;
mod enemy;
mod entity;
mod fs_app;
mod game;
mod game_data;
mod input;
mod model;
mod model_render;
mod packed_string;
mod player;
mod render;
mod script;
mod texture;
mod time;
mod trigger;
mod ui;
mod viewer;

fn find_bsp(fs: &fs_app::AppFs, name: Option<&str>) -> Option<String> {
    if let Some(name) = name {
        if fs.read(name).is_some() {
            return Some(name.to_string());
        }
    }

    let mut bsp_files: Vec<String> = fs
        .list_files()
        .filter(|p| p.ends_with(".sbp"))
        .map(|p| p.to_string())
        .collect();
    bsp_files.sort();

    if bsp_files.is_empty() {
        return None;
    }

    println!("Found {} BSP maps:", bsp_files.len());
    for (i, path) in bsp_files.iter().enumerate().take(10) {
        println!("  [{i}] {path}");
    }
    if bsp_files.len() > 10 {
        println!("  ... and {} more", bsp_files.len() - 10);
    }

    bsp_files
        .iter()
        .find(|p| p.contains("day1"))
        .cloned()
        .or_else(|| bsp_files.first().cloned())
}

fn init_sdl(title: &str) -> (sdl2::Sdl, sdl2::video::Window, sdl2::video::GLContext, sdl2::EventPump) {
    let sdl = sdl2::init().unwrap();
    let video = sdl.video().unwrap();

    let gl_attr = video.gl_attr();
    gl_attr.set_context_profile(sdl2::video::GLProfile::Core);
    gl_attr.set_context_version(3, 3);
    gl_attr.set_depth_size(24);
    gl_attr.set_stencil_size(8);

    let window = video
        .window(title, 1280, 720)
        .opengl()
        .resizable()
        .build()
        .unwrap();

    let gl_context = window.gl_create_context().unwrap();
    gl::load_with(|s| video.gl_get_proc_address(s) as *const _);

    unsafe {
        gl::Enable(gl::DEPTH_TEST);
        gl::Disable(gl::CULL_FACE);
        gl::ClearColor(0.1, 0.1, 0.15, 1.0);
    }

    let mut event_pump = sdl.event_pump().unwrap();
    sdl.mouse().set_relative_mouse_mode(true);
    event_pump.poll_iter().for_each(drop);

    (sdl, window, gl_context, event_pump)
}

fn print_usage() {
    eprintln!("Usage: qiye-remaster <app_path> [mode] [args...]");
    eprintln!();
    eprintln!("Modes:");
    eprintln!("  play [map]    Full gameplay (default)");
    eprintln!("  bsp  [map]    BSP map viewer — free-cam, map cycling");
    eprintln!("  obj  <name>   SOJ model viewer — orbit camera, animation");
    eprintln!("  npc  <name>   NPC/player viewer — state machine, movement");
    eprintln!();
    eprintln!("Examples:");
    eprintln!("  qiye-remaster ../qiye.app");
    eprintln!("  qiye-remaster ../qiye.app play \".\\day1\\0101.sbp\"");
    eprintln!("  qiye-remaster ../qiye.app bsp");
    eprintln!("  qiye-remaster ../qiye.app obj r_ken_a");
    eprintln!("  qiye-remaster ../qiye.app npc r_ken_a");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    let app_path = args.get(1).map(|s| s.as_str()).unwrap_or("../qiye.app");
    let mode = args.get(2).map(|s| s.as_str()).unwrap_or("play");

    // Check for help flag
    if app_path == "--help" || app_path == "-h" {
        print_usage();
        return;
    }

    println!("Loading {app_path}...");
    let fs = fs_app::AppFs::open(app_path);
    println!("PAK: {} files", fs.file_count());

    // List file extensions
    let mut ext_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for path in fs.list_files() {
        let ext = path
            .rsplit('.')
            .next()
            .map(|s| format!(".{s}"))
            .unwrap_or_else(|| "(none)".to_string());
        *ext_counts.entry(ext).or_default() += 1;
    }
    let mut ext_list: Vec<_> = ext_counts.into_iter().collect();
    ext_list.sort_by(|a, b| b.1.cmp(&a.1));
    println!("File types:");
    for (ext, count) in &ext_list {
        println!("  {ext}: {count}");
    }

    match mode {
        "play" => {
            let bsp_name = args.get(3).map(|s| s.as_str());
            let bsp_path = find_bsp(&fs, bsp_name).expect("No BSP file found in PAK");
            println!("Loading BSP: {bsp_path}");
            let bsp_data = fs.read(&bsp_path).unwrap();
            let bsp = bsp::Bsp::parse(bsp_data);

            let (sdl, window, _gl_context, mut event_pump) = init_sdl("七夜 Remaster");
            let sdl_audio = sdl.audio().unwrap();
            let audio_system = audio::AudioSystem::new(&sdl_audio);

            let mut game = game::Game::new(fs, bsp, &bsp_path, audio_system);
            game.run(&window, &mut event_pump);
        }
        "bsp" => {
            let bsp_name = args.get(3).map(|s| s.as_str());
            let bsp_path = find_bsp(&fs, bsp_name).expect("No BSP file found in PAK");
            println!("Loading BSP: {bsp_path}");
            let bsp_data = fs.read(&bsp_path).unwrap();
            let bsp = bsp::Bsp::parse(bsp_data);

            let (_sdl, window, _gl_context, mut event_pump) = init_sdl("七夜 — BSP Viewer");
            viewer::run_bsp(fs, &bsp, &bsp_path, &window, &mut event_pump);
        }
        "obj" => {
            let model_name = match args.get(3) {
                Some(n) => n.as_str(),
                None => {
                    eprintln!("Error: obj mode requires a model name");
                    print_usage();
                    return;
                }
            };
            let (_sdl, window, _gl_context, mut event_pump) = init_sdl(&format!("七夜 — OBJ: {model_name}"));
            viewer::run_obj(&fs, model_name, &window, &mut event_pump);
        }
        "npc" => {
            let model_name = match args.get(3) {
                Some(n) => n.as_str(),
                None => {
                    eprintln!("Error: npc mode requires a model name");
                    print_usage();
                    return;
                }
            };
            let (_sdl, window, _gl_context, mut event_pump) = init_sdl(&format!("七夜 — NPC: {model_name}"));
            viewer::run_npc(&fs, model_name, &window, &mut event_pump);
        }
        _ => {
            eprintln!("Unknown mode: {mode}");
            print_usage();
        }
    }
}
