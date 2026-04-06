mod bsp;
mod entity;
mod fs_app;
mod game;
mod input;
mod model;
mod model_render;
mod packed_string;
mod render;
mod script;
mod texture;
mod time;

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

fn main() {
    let app_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "../qiye.app".to_string());

    println!("Loading {app_path}...");
    let fs = fs_app::AppFs::open(&app_path);
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

    // Find and load BSP
    let bsp_name = std::env::args().nth(2);
    let bsp_path = find_bsp(&fs, bsp_name.as_deref()).expect("No BSP file found in PAK");
    println!("Loading BSP: {bsp_path}");
    let bsp_data = fs.read(&bsp_path).unwrap();
    let bsp = bsp::Bsp::parse(bsp_data);

    // SDL2 init
    let sdl = sdl2::init().unwrap();
    let video = sdl.video().unwrap();

    let gl_attr = video.gl_attr();
    gl_attr.set_context_profile(sdl2::video::GLProfile::Core);
    gl_attr.set_context_version(3, 3);
    gl_attr.set_depth_size(24);
    gl_attr.set_stencil_size(8);

    let window = video
        .window("七夜 Remaster", 1280, 720)
        .opengl()
        .resizable()
        .build()
        .unwrap();

    let _gl_context = window.gl_create_context().unwrap();
    gl::load_with(|s| video.gl_get_proc_address(s) as *const _);

    unsafe {
        gl::Enable(gl::DEPTH_TEST);
        gl::Disable(gl::CULL_FACE);
        gl::ClearColor(0.1, 0.1, 0.15, 1.0);
    }

    let mut event_pump = sdl.event_pump().unwrap();
    sdl.mouse().set_relative_mouse_mode(true);
    event_pump.poll_iter().for_each(drop);

    let mut game = game::Game::new(fs, &bsp, &bsp_path);
    game.run(&window, &mut event_pump);
}
