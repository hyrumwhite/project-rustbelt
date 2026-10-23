//! Halo: Combat Evolved levels rendered by Bevy Solari (real-time ray
//! tracing), with DLSS Ray Reconstruction when built with `--features dlss`.
//!
//! Phases 1-3 of ROADMAP.md: a retail Xbox map's structure BSPs, placed
//! objects and sky, with Halo's materials, lit by the level's sun and sky and
//! by its light-emitting surfaces. Phase 4: with `--bridge`, the objects and
//! camera of a running OpenCE game instead (see [`bridge`]).
//!
//! Built with `--features bundle` (`tools/package.py`), the executable
//! carries the game and the Rust simulation and, run, plays: it starts the
//! game hidden with every simulation system in Rust and draws it here (see
//! [`bundle`]).

#![allow(clippy::too_many_arguments, reason = "Bevy systems take their resources and queries as arguments")]

mod bridge;
mod bundle;
mod cpu_times;
mod decals;
mod effects;
mod lens_flares;
mod fog;
mod level;
mod lights;
mod material;
mod options;
mod overlay;
mod sky;
#[cfg(feature = "dlss")]
mod sky_overlay;
mod textures;

use argh::FromArgs;
use bevy::{
    camera::{CameraMainTextureUsages, Exposure},
    camera_controller::free_camera::{FreeCamera, FreeCameraPlugin},
    core_pipeline::Skybox,
    diagnostic::{Diagnostic, DiagnosticPath, DiagnosticsStore},
    light::EnvironmentMapLight,
    prelude::*,
    render::{diagnostic::RenderDiagnosticsPlugin, render_resource::TextureUsages, renderer::RenderDevice, settings::WgpuFeatures},
};
use bevy_solarik::{
    pathtracer::{Pathtracer, PathtracingPlugin},
    prelude::{SolarikLighting, SolarikPlugins, SolarikSkyLight},
};
use halo_map::{
    math::{halo_to_bevy, METERS_PER_WORLD_UNIT},
    scenario, CacheFile,
};
use level::{EmissiveSurface, LevelAssets, LevelBuilder, LevelStats, LightSource};
use material::{HaloEnvironmentMaterial, HaloMaterialsPlugin, HaloTransparentMaterial, NeutralDetail};
use std::path::PathBuf;

#[cfg(feature = "dlss")]
use bevy::{
    anti_alias::dlss::{
        Dlss, DlssPerfQualityMode, DlssProjectId, DlssRayReconstructionFeature, DlssRayReconstructionSupported,
    },
    render::camera::{MipBias, TemporalJitter},
};

/// Halo: CE levels with Bevy Solari ray tracing.
#[derive(FromArgs, Resource, Clone)]
struct Args {
    /// map name (b30, bloodgulch, ...) or path to a .map file (default b30;
    /// a build carrying the game starts at its menus without one)
    #[argh(positional)]
    map: Option<String>,
    /// folder holding the Xbox maps (default: $HALO_MAPS, else ./maps)
    #[argh(option)]
    maps_dir: Option<PathBuf>,
    /// load only this structure BSP (default: all)
    #[argh(option)]
    bsp: Option<usize>,
    /// use the reference pathtracer instead of real-time lighting
    #[argh(switch)]
    pathtracer: bool,
    /// start the camera at x,y,z[,yaw,pitch]: Halo world units and degrees
    /// (press P in the viewer to print the current view in this form; with
    /// --bridge or --opence, the camera starts detached from the game there)
    #[argh(option, from_str_fn(parse_camera))]
    camera: Option<[f32; 5]>,
    /// don't turn light fixtures into light sources
    #[argh(switch)]
    no_fixture_lights: bool,
    /// draw the light fixtures' emissive proxy spheres
    #[argh(switch)]
    show_light_proxies: bool,
    /// ignore Halo's reflection settings (every surface rough and non-metal)
    #[argh(switch)]
    matte: bool,
    /// leave out the scenario's objects (BSP geometry only)
    #[argh(switch)]
    no_objects: bool,
    /// save a screenshot here once the lighting has settled, then exit
    #[argh(option)]
    screenshot: Option<PathBuf>,
    /// how many frames to render before the screenshot (default 300)
    #[argh(option, default = "300")]
    screenshot_after: u32,
    /// with --screenshot, also save one every this many frames until then
    /// (`<name>_<frame>.png` beside it)
    #[argh(option)]
    screenshot_every: Option<u32>,
    /// with --screenshot-every, the frame to start at
    #[argh(option, default = "0")]
    screenshot_from: u32,
    /// move the camera forward and back this far (metres) along its view,
    /// once every two seconds of frames: the lighting's history in motion
    #[argh(option)]
    dolly: Option<f32>,
    /// draw a running OpenCE game (its objects and camera) instead of the
    /// map's placements; the map is the game's (start OpenCE with
    /// HALO_BEVY_BRIDGE=1)
    #[argh(switch)]
    bridge: bool,
    /// start this OpenCE (halo.exe, built from its bevy-bridge branch) on
    /// the map, hidden, and play it here (implies --bridge)
    #[argh(option)]
    opence: Option<PathBuf>,
    /// with --opence, show the game's own window too
    #[argh(switch)]
    show_game: bool,
    /// with --bridge, the name of the game's bridge (its
    /// HALO_BEVY_BRIDGE_NAME; default HaloBevyBridge)
    #[argh(option)]
    bridge_name: Option<String>,
    /// with --opence, start at the game's menus instead of on the map (the
    /// viewer follows the game to whatever level it plays)
    #[argh(switch)]
    menu: bool,
    /// with --opence, leave out the game's overlay (its HUD, effects and
    /// transparent surfaces) and draw the transparent surfaces here
    #[argh(switch)]
    no_overlay: bool,
    /// with --opence, the simulation's systems that run in Rust (its
    /// game.rust_simulation: "all", "none", or names; default "all" in a
    /// build carrying the game, else the game's own setting)
    #[argh(option)]
    simulation: Option<String>,
    /// in a build carrying the game, only view the map (no game)
    #[argh(switch)]
    viewer: bool,
    /// with --opence, leave glass and water in the game's overlay instead of
    /// drawing them here, ray traced
    #[argh(switch)]
    game_glass: bool,
    /// with --opence, leave the fog in the game's overlay instead of drawing
    /// it here
    #[argh(switch)]
    game_fog: bool,
    /// with --opence, leave the decals in the game's overlay instead of
    /// drawing them here
    #[argh(switch)]
    game_decals: bool,
    /// with --opence, leave the sprites (particles, weather, glows) in the
    /// game's overlay instead of drawing them here
    #[argh(switch)]
    game_effects: bool,
    /// with --opence, leave the transparent chicago and generic surfaces in
    /// the game's overlay instead of drawing them here
    #[argh(switch)]
    game_transparent: bool,
}

impl Args {
    /// Whether this viewer draws glass and water itself while the game shares
    /// its overlay: the game it starts leaves them out of the overlay
    /// (game.bevy_bridge_viewer_glass).
    fn viewer_glass(&self) -> bool {
        self.opence.is_some() && !self.no_overlay && !self.game_glass
    }

    /// Whether this viewer fogs its world itself while the game shares its
    /// overlay: the game it starts leaves its fog out of the overlay and
    /// shares the fog instead (game.bevy_bridge_viewer_fog).
    fn viewer_fog(&self) -> bool {
        self.opence.is_some() && !self.no_overlay && !self.game_fog
    }

    /// Whether this viewer draws the decals itself while the game shares its
    /// overlay: the game it starts shares them instead of drawing them
    /// (game.bevy_bridge_viewer_decals).
    fn viewer_decals(&self) -> bool {
        self.opence.is_some() && !self.no_overlay && !self.game_decals
    }

    /// Whether this viewer draws the sprites itself while the game shares
    /// its overlay: the game it starts shares them instead of drawing them
    /// (game.bevy_bridge_viewer_effects).
    fn viewer_effects(&self) -> bool {
        self.opence.is_some() && !self.no_overlay && !self.game_effects
    }

    /// Whether this viewer draws the transparent chicago and generic
    /// surfaces itself while the game shares its overlay: the game it starts
    /// leaves them out (game.bevy_bridge_viewer_transparent).
    fn viewer_transparent(&self) -> bool {
        self.opence.is_some() && !self.no_overlay && !self.game_transparent
    }
}

fn parse_camera(value: &str) -> Result<[f32; 5], String> {
    let numbers: Vec<f32> =
        value.split(',').map(|n| n.trim().parse::<f32>().map_err(|e| format!("{n}: {e}"))).collect::<Result<_, _>>()?;
    match numbers[..] {
        [x, y, z] => Ok([x, y, z, 0.0, 0.0]),
        [x, y, z, yaw] => Ok([x, y, z, yaw, 0.0]),
        [x, y, z, yaw, pitch] => Ok([x, y, z, yaw, pitch]),
        _ => Err("expected x,y,z[,yaw,pitch]".into()),
    }
}

/// A camera at a Halo position looking along a Halo yaw and pitch (radians).
fn halo_camera(position: [f32; 3], yaw: f32, pitch: f32) -> Transform {
    let eye = Vec3::from(halo_to_bevy(position, METERS_PER_WORLD_UNIT));
    let forward = Vec3::from(halo_to_bevy(
        [yaw.cos() * pitch.cos(), yaw.sin() * pitch.cos(), pitch.sin()],
        1.0,
    ));
    Transform::from_translation(eye).looking_to(forward, Vec3::Y)
}

impl Args {
    /// The map's name or path (b30 when none was given).
    fn map(&self) -> &str {
        self.map.as_deref().unwrap_or("b30")
    }

    /// The map file: the argument itself if it is a path, else `<name>.map`
    /// in the first maps folder that has it (see [`Args::map_dirs`]).
    fn map_path(&self) -> Result<PathBuf, String> {
        let direct = PathBuf::from(self.map());
        if direct.extension().is_some() || direct.is_file() {
            return Ok(direct);
        }
        let file = format!("{}.map", self.map());
        let dirs = self.map_dirs();
        dirs.iter().map(|dir| dir.join(&file)).find(|path| path.is_file()).ok_or_else(|| {
            let tried: Vec<String> = dirs.iter().map(|d| format!("  {}", d.display())).collect();
            format!("{file} not found in:\n{}", tried.join("\n"))
        })
    }

    /// `--maps-dir` alone when given; otherwise $HALO_MAPS, ./maps, a maps
    /// folder next to the executable, and where OpenCE's Windows build
    /// extracts the disc's maps (../OpenCE/build/windows/maps).
    fn map_dirs(&self) -> Vec<PathBuf> {
        if let Some(dir) = &self.maps_dir {
            return vec![dir.clone()];
        }
        let mut dirs: Vec<PathBuf> = std::env::var_os("HALO_MAPS").map(PathBuf::from).into_iter().collect();
        dirs.push(PathBuf::from("maps"));
        if let Some(exe_dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.join("maps"))) {
            dirs.push(exe_dir);
        }
        dirs.push(PathBuf::from("../OpenCE/build/windows/maps"));
        dirs
    }
}

/// Live-tunable light levels (Halo's radiosity powers have no physical unit).
#[derive(Resource, Clone, PartialEq)]
struct LightingSettings {
    /// Lux per unit of Halo sun power.
    sun_scale: f32,
    /// Environment light intensity per unit of Halo sky power.
    sky_scale: f32,
    /// Emissive luminance per unit of Halo radiosity power.
    emissive_scale: f32,
    exposure_ev100: f32,
}

impl Default for LightingSettings {
    fn default() -> Self {
        LightingSettings { sun_scale: 12_000.0, sky_scale: 2_500.0, emissive_scale: 400.0, exposure_ev100: 9.7 }
    }
}

#[derive(Resource, Clone)]
struct LevelSky(sky::SkyLighting);

fn main() -> AppExit {
    let mut args: Args = argh::from_env();
    let mut link = None;
    let mut game_process = None;
    let mut overlay = false;
    let mut bridge_name = args.bridge_name.clone().unwrap_or_else(|| halo_bridge::DEFAULT_NAME.to_string());
    // (a build carrying the game plays it: started hidden, every system in
    // Rust, at its menus unless a map was named)
    let mut simulation_dll = None;
    if bundle::EMBEDDED && args.opence.is_none() && !args.bridge && !args.viewer {
        match bundle::unpack() {
            Ok(Some(runtime)) => {
                eprintln!("the game and the Rust simulation: {}", runtime.dir.display());
                args.opence = Some(runtime.game);
                simulation_dll = Some(runtime.simulation);
                args.menu |= args.map.is_none();
                args.simulation.get_or_insert_with(|| "all".to_string());
            }
            Ok(None) => {}
            Err(e) => {
                eprintln!("could not unpack the game: {e}");
                return AppExit::error();
            }
        }
    } else if bundle::EMBEDDED && cfg!(feature = "dlss") {
        // (DLSS's libraries all the same, for the viewer and another game)
        if let Err(e) = bundle::unpack() {
            eprintln!("could not unpack DLSS: {e}");
        }
    }
    // (a build run in place: DLSS's libraries in the SDK it was built with)
    #[cfg(feature = "dlss")]
    if std::env::var_os("DLSS_SDK").is_none() {
        if let Some(sdk) = option_env!("DLSS_SDK") {
            // SAFETY: at start-up, before any other thread runs
            unsafe { std::env::set_var("DLSS_SDK", sdk) };
        }
    }
    if let Some(exe) = args.opence.clone() {
        args.bridge = true;
        let map_path = match args.map_path() {
            Ok(path) => path,
            Err(e) => {
                eprintln!("{e}");
                eprintln!("pass --maps-dir <folder>, or set HALO_MAPS to the folder holding the Xbox maps, e.g.");
                eprintln!("  setx HALO_MAPS \"C:\\path\\to\\maps\"   (then open a new terminal)");
                return AppExit::error();
            }
        };
        // (absolute: the game runs from its own folder)
        let exe = match std::fs::canonicalize(&exe) {
            Ok(exe) => exe,
            Err(e) => {
                eprintln!("could not find OpenCE at {}: {e}", exe.display());
                return AppExit::error();
            }
        };
        match {
            bridge_name = bridge::own_bridge_name();
            let simulation = args.simulation.as_deref().map(|systems| bridge::Simulation { systems, dll: simulation_dll.as_deref() });
            bridge::launch_game(&exe, &map_path, !args.menu, &bridge_name, args.show_game, !args.no_overlay, args.viewer_glass(), args.viewer_fog(), args.viewer_decals(), args.viewer_effects(), args.viewer_transparent(), simulation)
        } {
            // (stops the game when the viewer exits, or gives up on it)
            Ok(child) => game_process = Some(bridge::GameProcess(child)),
            Err(e) => {
                eprintln!("could not start {}: {e}", exe.display());
                return AppExit::error();
            }
        }
    }
    if args.bridge {
        let Some((bridge, snapshot)) = bridge::wait_for_game(&bridge_name, std::time::Duration::from_secs(300)) else {
            eprintln!("no OpenCE game shared a frame; start it with HALO_BEVY_BRIDGE=1 and load a level");
            return AppExit::error();
        };
        eprintln!("bridged to OpenCE: {} ({} objects)", snapshot.scenario_name, snapshot.objects.len());
        args.map = Some(snapshot.map_name().to_string());
        overlay = bridge.overlay_enabled();
        link = Some(bridge::BridgeLink::new(bridge, snapshot));
    }
    let path = match args.map_path() {
        Ok(path) => path,
        Err(e) => {
            eprintln!("{e}");
            eprintln!("pass --maps-dir <folder>, or set HALO_MAPS to the folder holding the Xbox maps, e.g.");
            eprintln!("  setx HALO_MAPS \"C:\\path\\to\\maps\"   (then open a new terminal)");
            return AppExit::error();
        }
    };
    let cache = match CacheFile::open(&path) {
        Ok(cache) => cache,
        Err(e) => {
            eprintln!("could not open {}: {e}", path.display());
            eprintln!("pass a map name and --maps-dir, or set HALO_MAPS to the folder holding the Xbox maps");
            return AppExit::error();
        }
    };
    // the options saved last time (F10)
    let saved = if args.pathtracer { None } else { options::SavedSettings::load() };

    let mut app = App::new();

    #[cfg(feature = "dlss")]
    app.insert_resource(DlssProjectId(bevy::asset::uuid::uuid!("3f1d6a2e-8a4b-4f5e-9c3d-1b2a7e6f0d91")));

    // (unfocused too, at full speed: Bevy's default updates an unfocused
    // window 60 times a second, which held scripted captures, and perhaps a
    // window Windows doesn't count as focused, there)
    // (`HALO_RT_UNFOCUSED_60=1`: unfocused, 60 frames a second, as before:
    // scripted runs then show the same game tick at the same frame, for
    // comparing two builds frame by frame)
    let unfocused_mode = if std::env::var_os("HALO_RT_UNFOCUSED_60").is_some() {
        bevy::winit::UpdateMode::reactive_low_power(std::time::Duration::from_secs_f64(1.0 / 60.0))
    } else {
        bevy::winit::UpdateMode::Continuous
    };
    app.insert_resource(bevy::winit::WinitSettings { focused_mode: bevy::winit::UpdateMode::Continuous, unfocused_mode });
    app.add_plugins((default_plugins(&args, overlay), SolarikPlugins, FreeCameraPlugin, RenderDiagnosticsPlugin, HaloMaterialsPlugin))
        .insert_resource(args.clone())
        .insert_resource(overlay::OverlayMode(overlay))
        .insert_resource(saved.as_ref().map_or_else(LightingSettings::default, |s| s.lighting.clone()))
        .insert_resource(MapFile(Some(cache)))
        .add_systems(Startup, (setup_level, setup_camera, setup_ui).chain())
        .add_systems(Update, (start_camera, sync_sky_light))
        .init_resource::<ResetHistory>()
        .init_resource::<level::PeriodicTables>()
        .add_systems(Update, (level::animate_self_illumination.after(apply_lighting), level::animate_transparent))
        .add_systems(PostUpdate, level::follow_camera.before(TransformSystems::Propagate))
        .add_systems(PostUpdate, reset_history_on_cuts.before(TransformSystems::Propagate))
        .add_systems(PostUpdate, level::sync_traced_visibility.after(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate));

    app.add_systems(Update, capture);
    app.init_resource::<CaptureFrames>().add_systems(First, count_capture_frames);
    if std::env::var_os("HALO_RT_CACHE_LOG").is_some() {
        app.add_systems(PostUpdate, log_world_cache);
    }
    if std::env::var_os("HALO_RT_GPU_LOG").is_some() {
        app.add_systems(Last, log_gpu_times).add_plugins(cpu_times::CpuTimesPlugin);
    }
    if args.dolly.is_some() {
        app.add_systems(Update, dolly_camera);
    }
    if args.screenshot.is_some() {
        app.add_systems(Update, screenshot_and_exit);
    }
    if let Some(mut link) = link {
        link.sync_to_overlay = overlay;
        app.insert_resource(link)
            .add_plugins((bridge::BridgePlugin, lights::DynamicLightsPlugin))
            .add_plugins(bevy::diagnostic::SystemInformationDiagnosticsPlugin)
            .add_systems(PreUpdate, follow_level)
            .add_systems(Last, log_heartbeat);
        if args.camera.is_some() {
            app.insert_resource(bridge::FollowGame(false));
        }
        if overlay {
            app.add_plugins(overlay::OverlayPlugin);
            if args.viewer_fog() {
                app.add_plugins(fog::FogPlugin);
                if args.viewer_glass() {
                    app.insert_resource(fog::ViewerWater);
                }
            }
            if args.viewer_decals() {
                app.add_plugins(decals::DecalsPlugin);
            }
            if args.viewer_effects() {
                app.add_plugins(effects::EffectsPlugin);
                // (the lens flares are added in the fog pass; without it the
                // game draws them)
                if args.viewer_fog() {
                    app.add_plugins(lens_flares::LensFlaresPlugin);
                }
            }
        }
    }
    if let Some(game_process) = game_process {
        app.insert_resource(game_process);
    }

    if args.pathtracer {
        app.add_plugins(PathtracingPlugin);
    } else {
        app.add_plugins(options::OptionsPlugin);
        if let Some(saved) = saved {
            app.insert_resource(options::PendingSettings(saved));
        }
        app.add_systems(
            Update,
            (
                (adjust_lighting.run_if(bridge::not_forwarding), apply_lighting, follow_camera_sky).chain(),
                print_camera.run_if(bridge::not_forwarding),
            ),
        )
            .add_systems(PostUpdate, (update_control_text, update_performance_text));
        #[cfg(feature = "dlss")]
        app.add_systems(Update, (toggle_dlss_rr, cycle_dlss_quality).run_if(bridge::not_forwarding));
        #[cfg(feature = "dlss")]
        app.add_plugins(sky_overlay::SkyOverlayPlugin);
    }

    app.run()
}

/// Asks for the lighting's (and DLSS's) saved past frames to be dropped
/// this frame: the level or its active BSP changed, so they no longer show
/// what is there. Camera cuts ask for it themselves (`reset_history_on_cuts`).
#[derive(Resource, Default)]
pub struct ResetHistory(pub bool);

/// A camera that moved this far in a frame (metres) cut; or turned this far
/// (radians): a cutscene's next shot, a respawn, a teleport, the free camera
/// taken or given back. Moving the fastest vehicle or turning the fastest
/// flick goes a small part of either in a frame.
const CUT_DISTANCE: f32 = 3.0;
const CUT_ANGLE: f32 = std::f32::consts::FRAC_PI_4;

/// Drops the lighting's temporal history (Solarik's reservoirs and world
/// cache, the pathtracer's accumulation, DLSS's past frames) at a camera
/// cut or when `ResetHistory` asks: reprojected, they would ghost the old
/// view into the new for a moment.
fn reset_history_on_cuts(
    mut reset: ResMut<ResetHistory>,
    mut last: Local<Option<(Vec3, Vec3)>>,
    camera: Single<(&Transform, Option<&mut SolarikLighting>, Option<&mut Pathtracer>), With<Camera3d>>,
    #[cfg(feature = "dlss")] mut dlss: Query<&mut Dlss<DlssRayReconstructionFeature>, With<Camera3d>>,
) {
    let (transform, lighting, pathtracer) = camera.into_inner();
    let now = (transform.translation, *transform.forward());
    let cut = last.is_some_and(|(position, forward)| {
        position.distance(now.0) > CUT_DISTANCE || forward.angle_between(now.1) > CUT_ANGLE
    });
    *last = Some(now);
    if !cut && !reset.0 {
        return;
    }
    debug!("the lighting's history dropped: {}", if cut { "a camera cut" } else { "the level changed" });
    reset.0 = false;
    if let Some(mut lighting) = lighting {
        lighting.reset = true;
    }
    if let Some(mut pathtracer) = pathtracer {
        pathtracer.reset = true;
    }
    #[cfg(feature = "dlss")]
    for mut dlss in &mut dlss {
        dlss.reset = true;
    }
}

/// The open cache file, consumed by `setup_level`.
#[derive(Resource)]
struct MapFile(Option<CacheFile>);

fn default_plugins(args: &Args, overlay: bool) -> impl PluginGroup {
    use bevy::render::{
        settings::{Backends, WgpuSettings},
        RenderPlugin,
    };
    let title = format!("Halo RT - {}", args.map());
    // (a scripted capture opens behind the window in use, without taking
    // the keyboard)
    let mut window = Window { title, focused: args.screenshot.is_none(), ..default() };
    // (`HALO_RT_NO_VSYNC=1`: frames as fast as they come, for timing)
    if std::env::var_os("HALO_RT_NO_VSYNC").is_some() {
        window.present_mode = bevy::window::PresentMode::AutoNoVsync;
    }
    if overlay {
        // the game's overlay is drawn at this shape (overlay::WINDOW_SIZE)
        window.resolution = overlay::WINDOW_SIZE.into();
        window.resizable = false;
    }
    DefaultPlugins
        .set(WindowPlugin { primary_window: Some(window), ..default() })
        .set(RenderPlugin {
            render_creation: WgpuSettings {
                // DLSS only works on Vulkan; Solari's ray queries are best
                // supported there too
                backends: Some(Backends::VULKAN),
                ..default()
            }
            .into(),
            ..default()
        })
}

/// The assets a level's meshes and materials go into.
#[derive(bevy::ecs::system::SystemParam)]
struct LevelAssetParams<'w> {
    render_device: Res<'w, RenderDevice>,
    meshes: ResMut<'w, Assets<Mesh>>,
    materials: ResMut<'w, Assets<StandardMaterial>>,
    environment_materials: ResMut<'w, Assets<HaloEnvironmentMaterial>>,
    transparent_materials: ResMut<'w, Assets<HaloTransparentMaterial>>,
    sky_water: ResMut<'w, Assets<material::SkyWaterMaterial>>,
    water_ripples: ResMut<'w, bevy_solarik::water_ripples::WaterRippleMaterials>,
    images: ResMut<'w, Assets<Image>>,
    inverse_bindposes: ResMut<'w, Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>,
    neutral_detail: Res<'w, NeutralDetail>,
    glass_sky_mask: Res<'w, material::GlassSkyMaskMaterial>,
}

/// The parent of the loaded level's entities.
#[derive(Resource)]
struct LevelRoot(Entity);

fn setup_level(
    mut commands: Commands,
    mut map: ResMut<MapFile>,
    args: Res<Args>,
    overlay: Res<overlay::OverlayMode>,
    settings: Res<LightingSettings>,
    mut params: LevelAssetParams,
) {
    let cache = map.0.take().expect("level loads once");
    let root = commands.spawn((Name::new("level"), Transform::default(), Visibility::default())).id();
    commands.insert_resource(LevelRoot(root));
    load_level(cache, root, &args, overlay.0, &settings, &mut commands, &mut params);
}

/// Loads `cache`'s level under `root`: its BSPs, the lights of its objects
/// (or all its objects, unbridged), its sky and sun; and the resources that
/// describe it (`LevelStats`, `LevelSky`, `SkyCubemap`, `CameraStart`, and
/// bridged, `BridgeLevel`).
fn load_level(
    cache: CacheFile,
    root: Entity,
    args: &Args,
    overlay: bool,
    settings: &LightingSettings,
    commands: &mut Commands,
    params: &mut LevelAssetParams,
) {
    let level_sky = sky::SkyLighting::from_map(&cache);
    let cache = std::sync::Arc::new(cache);
    // (the last level's water panes' ripples)
    params.water_ripples.0.clear();
    let mut assets = LevelAssets {
        meshes: &mut params.meshes,
        materials: &mut params.materials,
        environment_materials: &mut params.environment_materials,
        transparent_materials: &mut params.transparent_materials,
        sky_water: Some(&mut params.sky_water),
        water_ripples: Some(&mut params.water_ripples),
        images: &mut params.images,
        inverse_bindposes: &mut params.inverse_bindposes,
        neutral_detail: params.neutral_detail.0.clone(),
        flat_normal: Handle::default(),
    };
    let mut commands = commands;
    assets.flat_normal = assets.images.add(material::flat_normal_image());
    let bc_supported = params.render_device.features().contains(WgpuFeatures::TEXTURE_COMPRESSION_BC);
    let start = std::time::Instant::now();
    let mut builder = LevelBuilder::new(cache.clone(), bc_supported);
    builder.root = Some(root);
    builder.glass_sky_mask = Some(params.glass_sky_mask.clone());
    builder.fixture_lights = !args.no_fixture_lights;
    builder.show_light_proxies = args.show_light_proxies;
    builder.matte = args.matte;
    // (the game's overlay has them; but glass and water, when this viewer
    // draws them)
    builder.skip_transparent = overlay && !args.viewer_transparent();
    builder.draw_blended = !overlay || args.viewer_glass();
    if let Err(e) = builder.spawn_bsps(args.bsp, !args.pathtracer, &mut commands, &mut assets) {
        error!("loading {}: {e}", cache.header.name);
    }
    // a bridged game places the objects itself; the map still gives the
    // light fixtures' lights
    if (!args.no_objects || args.bridge)
        && let Err(e) = builder.spawn_objects(!args.pathtracer, args.bridge, &mut commands, &mut assets)
    {
        error!("placing objects: {e}");
    }
    let sky_model = halo_map::sky::skies(&cache).ok().and_then(|s| s.into_iter().find_map(|s| s.model)).and_then(|m| m.datum);
    if let Some(model) = sky_model {
        if let Err(e) = builder.spawn_sky(model, &mut commands, &mut assets) {
            error!("sky: {e}");
        }
        match sky::sky_model_cubemap(&cache, model, assets.images) {
            Ok(Some(cubemaps)) => commands.insert_resource(SkyCubemap(cubemaps.lighting, cubemaps.background)),
            Ok(None) => commands.remove_resource::<SkyCubemap>(),
            Err(e) => {
                error!("sky environment map: {e}");
                commands.remove_resource::<SkyCubemap>();
            }
        }
    } else {
        commands.remove_resource::<SkyCubemap>();
    }
    if let Some((direction, color, power)) = level_sky.sun {
        commands.spawn((
            Name::new("sun"),
            DirectionalLight {
                color,
                illuminance: power * settings.sun_scale,
                // Solari replaces shadow maps
                shadow_maps_enabled: false,
                ..default()
            },
            Transform::default().looking_to(-direction, Vec3::Y),
            ChildOf(root),
        ));
    }
    commands.insert_resource(LevelSky(level_sky));
    let stats = builder.stats.clone();
    info!(
        "{}: {} bsps ({} triangles), {} objects ({} triangles, {} failed), {} fixture lights, {} meshes, {} emissive, {} transparent ({} not drawn), {} texture failures ({:.2?})",
        stats.map_name,
        stats.bsps,
        stats.triangles,
        stats.objects,
        stats.object_triangles,
        stats.object_failures,
        stats.lights,
        stats.meshes,
        stats.emissive_meshes,
        stats.transparent_meshes,
        stats.skipped_transparent,
        stats.texture_failures,
        start.elapsed()
    );

    // Where the first player starts, facing as they do.
    let start = scenario::player_starts(&cache).ok().and_then(|s| s.into_iter().find(|s| args.bsp.is_none_or(|b| s.bsp_index < 0 || s.bsp_index as usize == b)));
    let transform = match (args.camera, start) {
        (Some([x, y, z, yaw, pitch]), _) => halo_camera([x, y, z], yaw.to_radians(), pitch.to_radians()),
        (None, Some(start)) => {
            let [x, y, z] = start.position;
            info!("player start {x},{y},{z} facing {}", start.facing.to_degrees());
            halo_camera([x, y, z + 0.62], start.facing, 0.0)
        }
        // no start in this BSP: look across it from above its centre
        (None, None) => bsp_overview(&cache, args.bsp.unwrap_or(0)).unwrap_or_else(|| Transform::from_xyz(0.0, 10.0, 0.0)),
    };
    commands.insert_resource(CameraStart(transform));
    commands.insert_resource(stats);

    if args.bridge {
        commands.insert_resource(bridge::BridgeLevel { builder, raster: !args.pathtracer });
    }
}

#[derive(Resource)]
struct CameraStart(Transform);

/// The sky model rendered into cubemaps: the environment light's and the
/// skybox's (see [`sky::SkyModelCubemaps`]).
#[derive(Resource)]
struct SkyCubemap(Handle<Image>, Handle<Image>);

fn bsp_overview(cache: &CacheFile, index: usize) -> Option<Transform> {
    let reference = halo_map::bsp::references(cache).ok()?.into_iter().find(|r| r.index == index)?;
    let [x, y, z] = halo_map::bsp::Bsp::load(cache, &reference).ok()?.world_bounds().ok()?;
    let centre = [(x[0] + x[1]) / 2.0, (y[0] + y[1]) / 2.0, (z[0] + z[1]) / 2.0];
    Some(Transform::from_translation(Vec3::from(halo_to_bevy(centre, METERS_PER_WORLD_UNIT))))
}

/// Every 10 seconds, a line in the log: the frames drawn, the game's tick,
/// and how many entities and assets there are, with the process's memory. A
/// freeze shows where they stop; a leak, what grows.
/// (`HALO_RT_GPU_LOG=1`) Every 5 seconds, the GPU time of each render pass
/// Bevy's render diagnostics time (milliseconds, averaged), and their sum.
fn log_gpu_times(store: Res<bevy::diagnostic::DiagnosticsStore>, mut last: Local<Option<std::time::Instant>>) {
    if last.is_some_and(|last| last.elapsed().as_secs_f32() < 5.0) {
        return;
    }
    *last = Some(std::time::Instant::now());
    let mut times: Vec<(String, f64)> = store
        .iter()
        .filter(|d| d.path().as_str().ends_with("/elapsed_gpu"))
        // (not those left from frames long gone: a span timed only on the
        // frames it runs keeps its last average, BLAS builds at load)
        .filter(|d| d.measurement().is_some_and(|m| m.time.elapsed().as_secs_f32() < 1.0))
        .filter_map(|d| Some((d.path().as_str().trim_end_matches("/elapsed_gpu").to_string(), d.average()?)))
        .collect();
    times.sort_by(|a, b| b.1.total_cmp(&a.1));
    let total: f64 = times.iter().map(|t| t.1).sum();
    let list: Vec<String> = times.iter().filter(|t| t.1 >= 0.02).map(|(name, ms)| format!("{name} {ms:.2}")).collect();
    info!("gpu {total:.2} ms: {}", list.join(", "));
}

fn log_heartbeat(world: &World, mut last: Local<Option<std::time::Instant>>) {
    if last.is_some_and(|last| last.elapsed().as_secs_f32() < 10.0) {
        return;
    }
    *last = Some(std::time::Instant::now());
    let frames = world.get_resource::<bevy::diagnostic::FrameCount>().map_or(0, |f| f.0);
    let tick = world.get_resource::<bridge::BridgeLink>().and_then(|l| l.latest.as_ref()).map_or(0, |s| s.game_tick);
    let count = |n: Option<usize>| n.unwrap_or(0);
    let meshes = count(world.get_resource::<Assets<Mesh>>().map(|a| a.len()));
    let images = count(world.get_resource::<Assets<Image>>().map(|a| a.len()));
    let standard = count(world.get_resource::<Assets<StandardMaterial>>().map(|a| a.len()));
    let memory = world
        .get_resource::<bevy::diagnostic::DiagnosticsStore>()
        .and_then(|d| d.get(&bevy::diagnostic::SystemInformationDiagnosticsPlugin::PROCESS_MEM_USAGE))
        .and_then(|d| d.value());
    info!(
        "heartbeat: frame {frames}, tick {tick}, {} entities, {meshes} meshes, {images} images, {standard} materials, memory {:.2} GiB",
        world.entities().count_spawned(),
        memory.unwrap_or(0.0)
    );
}

fn setup_camera(
    mut commands: Commands,
    args: Res<Args>,
    start: Res<CameraStart>,
    level_sky: Res<LevelSky>,
    settings: Res<LightingSettings>,
    sky_cubemap: Option<Res<SkyCubemap>>,
    mut images: ResMut<Assets<Image>>,
    #[cfg(feature = "dlss")] dlss_rr_supported: Option<Res<DlssRayReconstructionSupported>>,
) {
    let mut camera = commands.spawn((
        // (the fog reads the main pass's depth)
        Camera3d { depth_texture_usages: (TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING).into(), ..default() },
        // (inactive for its first frames: start_camera)
        Camera { clear_color: ClearColorConfig::Custom(Color::BLACK), is_active: args.pathtracer, ..default() },
        start.0,
        // Msaa::Off and STORAGE_BINDING main textures are required by Solari
        CameraMainTextureUsages::default().with(TextureUsages::STORAGE_BINDING),
        Msaa::Off,
        Exposure { ev100: settings.exposure_ev100 },
    ));
    if args.pathtracer {
        camera.insert(Pathtracer::default());
    } else {
        // a game's scene never stops changing (lights culled with the view,
        // flashing and moving, its objects in motion): the lighting's history
        // follows it instead of starting over at each change, which left
        // the indirect light in dim rooms smoky (HALO_RT_SCENE_RESETS=1 for
        // Solarik's own, for comparison)
        let dynamic_scene = std::env::var_os("HALO_RT_SCENE_RESETS").is_none();
        camera.insert(SolarikLighting { dynamic_scene, ..default() });
    }
    // a bridged camera follows the game's until F8 detaches it (or, with
    // --camera, starts detached there)
    // (not for a scripted capture, which nothing should move)
    if (!args.bridge || args.camera.is_some()) && args.screenshot.is_none() {
        camera.insert(FreeCamera { walk_speed: 6.0, run_speed: 25.0, ..default() });
    }

    if let Some(sky) = sky_environment(&level_sky, sky_cubemap.as_deref(), &settings, &mut images) {
        camera.insert(sky);
    }

    #[cfg(feature = "dlss")]
    if dlss_rr_supported.is_some() && std::env::var_os("HALO_RT_NO_DLSS").is_none() {
        camera.insert(Dlss::<DlssRayReconstructionFeature>::default());
    }

}
/// The camera's environment light and skybox for the level's sky.
fn sky_environment(
    level_sky: &LevelSky,
    sky_cubemap: Option<&SkyCubemap>,
    settings: &LightingSettings,
    images: &mut Assets<Image>,
) -> Option<(EnvironmentMapLight, Skybox)> {
    let (color, power) = level_sky.0.sky?;
    let (cubemap, background) = match sky_cubemap {
        Some(sky) => (sky.0.clone(), sky.1.clone()),
        None => {
            let gradient = sky::gradient_cubemap(color, images);
            (gradient.clone(), gradient)
        }
    };
    Some((
        EnvironmentMapLight {
            diffuse_map: cubemap.clone(),
            specular_map: cubemap.clone(),
            intensity: power * settings.sky_scale,
            ..default()
        },
        Skybox { image: Some(background), brightness: power * settings.sky_scale, ..default() },
    ))
}

/// Bridged: when the game changes level (a new map, the menus, the next
/// game of a multiplayer session), unloads this one and loads the game's.
fn follow_level(
    mut commands: Commands,
    link: Res<bridge::BridgeLink>,
    args: Res<Args>,
    overlay: Res<overlay::OverlayMode>,
    mut settings: ResMut<LightingSettings>,
    mut root: ResMut<LevelRoot>,
    stats: Res<LevelStats>,
    mut objects: ResMut<bridge::BridgeObjects>,
    camera: Single<Entity, With<Camera3d>>,
    mut window: Single<&mut Window, With<bevy::window::PrimaryWindow>>,
    mut params: LevelAssetParams,
    mut failed: Local<Option<String>>,
    mut reset: ResMut<ResetHistory>,
) {
    let Some(snapshot) = &link.latest else { return };
    let map = snapshot.map_name();
    if map.is_empty() || map.eq_ignore_ascii_case(&stats.map_name) || failed.as_deref() == Some(map) {
        return;
    }
    let level_args = Args { map: Some(map.to_string()), ..args.clone() };
    let cache = match level_args.map_path().map_err(|e| e.to_string()).and_then(|p| CacheFile::open(&p).map_err(|e| e.to_string())) {
        Ok(cache) => cache,
        Err(e) => {
            // (the old level stays; the game plays on)
            error!("the game changed to {}, which can't be loaded: {e}", snapshot.scenario_name);
            *failed = Some(map.to_string());
            return;
        }
    };
    info!("the game changed level: {} -> {}", stats.map_name, snapshot.scenario_name);
    *failed = None;
    reset.0 = true;
    commands.entity(root.0).despawn();
    objects.clear();
    root.0 = commands.spawn((Name::new("level"), Transform::default(), Visibility::default())).id();
    load_level(cache, root.0, &args, overlay.0, &settings, &mut commands, &mut params);
    window.title = format!("Halo RT - {map}");
    // the camera's sky, once the new level's resources are in
    let camera = *camera;
    commands.queue(move |world: &mut World| {
        let sky = world.resource::<LevelSky>().clone();
        let settings = world.resource::<LightingSettings>().clone();
        let cubemap = world.get_resource::<SkyCubemap>().map(|c| SkyCubemap(c.0.clone(), c.1.clone()));
        let environment =
            world.resource_scope(|_, mut images: Mut<Assets<Image>>| sky_environment(&sky, cubemap.as_ref(), &settings, &mut images));
        let mut camera = world.entity_mut(camera);
        match environment {
            Some(environment) => {
                camera.insert(environment);
            }
            None => {
                camera.remove::<(EnvironmentMapLight, Skybox)>();
            }
        }
    });
    // (applies the emissive levels to the new level's materials)
    settings.set_changed();
}


/// `[`/`]` emissive, `-`/`=` sun, `9`/`0` sky, `,`/`.` exposure.
fn adjust_lighting(
    keys: Res<ButtonInput<KeyCode>>,
    mut settings: ResMut<LightingSettings>,
) {
    let step = 1.25;
    let scale = |up: KeyCode, down: KeyCode, value: &mut f32| {
        if keys.just_pressed(up) {
            *value *= step;
        }
        if keys.just_pressed(down) {
            *value /= step;
        }
    };
    let mut s = settings.clone();
    scale(KeyCode::BracketRight, KeyCode::BracketLeft, &mut s.emissive_scale);
    scale(KeyCode::Equal, KeyCode::Minus, &mut s.sun_scale);
    scale(KeyCode::Digit0, KeyCode::Digit9, &mut s.sky_scale);
    if keys.just_pressed(KeyCode::Period) {
        s.exposure_ev100 += 0.5;
    }
    if keys.just_pressed(KeyCode::Comma) {
        s.exposure_ev100 -= 0.5;
    }
    if keys.any_just_pressed([
        KeyCode::BracketRight,
        KeyCode::BracketLeft,
        KeyCode::Equal,
        KeyCode::Minus,
        KeyCode::Digit0,
        KeyCode::Digit9,
        KeyCode::Period,
        KeyCode::Comma,
    ]) {
        *settings = s;
    }
}

fn apply_lighting(
    settings: Res<LightingSettings>,
    level_sky: Res<LevelSky>,
    surfaces: Query<(&EmissiveSurface, &MeshMaterial3d<StandardMaterial>)>,
    environment_surfaces: Query<(&EmissiveSurface, &MeshMaterial3d<HaloEnvironmentMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut environment_materials: ResMut<Assets<HaloEnvironmentMaterial>>,
    mut suns: Query<&mut DirectionalLight>,
    mut cameras: Query<(&mut Exposure, Option<&mut EnvironmentMapLight>, Option<&mut Skybox>)>,
    mut point_lights: Query<(&LightSource, &mut PointLight)>,
) {
    if !settings.is_changed() {
        return;
    }
    for (source, mut light) in &mut point_lights {
        (light.color, light.intensity) = source.color_and_intensity(settings.emissive_scale);
    }
    let emissive = |surface: &EmissiveSurface| {
        let radiance = surface.radiance * settings.emissive_scale;
        LinearRgba::rgb(radiance.x, radiance.y, radiance.z)
    };
    for (surface, material) in &surfaces {
        if let Some(mut material) = materials.get_mut(&material.0) {
            material.emissive = emissive(surface);
        }
    }
    for (surface, material) in &environment_surfaces {
        if let Some(mut material) = environment_materials.get_mut(&material.0) {
            material.base.emissive = emissive(surface);
        }
    }
    if let Some((_, _, power)) = level_sky.0.sun {
        for mut sun in &mut suns {
            sun.illuminance = power * settings.sun_scale;
        }
    }
    for (mut exposure, environment, skybox) in &mut cameras {
        exposure.ev100 = settings.exposure_ev100;
        if let Some((_, power)) = level_sky.0.sky {
            if let Some(mut environment) = environment {
                environment.intensity = power * settings.sky_scale;
            }
            if let Some(mut skybox) = skybox {
                skybox.brightness = power * settings.sky_scale;
            }
        }
    }
}

/// The camera's skybox dark where the game's camera is in a cluster with no
/// sky ([`bridge::CameraSky`]), as the game draws none there; the sky's light
/// is left as it is.
fn follow_camera_sky(
    settings: Res<LightingSettings>,
    level_sky: Res<LevelSky>,
    camera_sky: Option<Res<bridge::CameraSky>>,
    mut skyboxes: Query<&mut Skybox>,
) {
    let Some(camera_sky) = camera_sky else { return };
    if !camera_sky.is_changed() && !settings.is_changed() {
        return;
    }
    let Some((_, power)) = level_sky.0.sky else { return };
    let brightness = if camera_sky.none { 0.0 } else { power * settings.sky_scale };
    for mut skybox in &mut skyboxes {
        if skybox.brightness != brightness {
            skybox.brightness = brightness;
        }
    }
}

/// Solarik lights the rays that leave the level from `SolarikSkyLight`,
/// where Solari took the camera's environment light: it gets the same
/// cubemap and intensity (none without one).
fn sync_sky_light(
    mut commands: Commands,
    camera: Query<Option<Ref<EnvironmentMapLight>>, With<Camera3d>>,
    sky: Option<Res<SolarikSkyLight>>,
) {
    let Some(environment) = camera.iter().next() else { return };
    match environment {
        Some(environment) if environment.is_changed() || sky.as_ref().is_none_or(|s| s.image.is_none()) => {
            commands.insert_resource(SolarikSkyLight {
                image: Some(environment.specular_map.clone()),
                intensity: environment.intensity,
            });
        }
        None if sky.is_some_and(|s| s.image.is_some()) => commands.insert_resource(SolarikSkyLight { image: None, intensity: 1.0 }),
        _ => {}
    }
}

/// The camera as a `--camera` argument.
fn camera_argument(camera: &Transform) -> String {
    let p = camera.translation / METERS_PER_WORLD_UNIT;
    let f = camera.forward();
    // Bevy (x, y, z) is Halo (x, -z, y)
    let (hx, hy, hz) = (f.x, -f.z, f.y);
    let yaw = hy.atan2(hx).to_degrees();
    let pitch = hz.clamp(-1.0, 1.0).asin().to_degrees();
    format!("--camera {:.2},{:.2},{:.2},{yaw:.1},{pitch:.1}", p.x, -p.z, p.y)
}

/// P: log the camera as a `--camera` argument.
fn print_camera(keys: Res<ButtonInput<KeyCode>>, camera: Single<&Transform, With<Camera3d>>) {
    if keys.just_pressed(KeyCode::KeyP) {
        info!("{}", camera_argument(&camera));
    }
}

/// F9, in play too: save the window's picture (in `captures` beside the
/// program) and log where it was taken: the level, the game's tick and the
/// camera, as a `--camera` argument.
fn capture(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    camera: Single<&Transform, With<Camera3d>>,
    link: Option<Res<bridge::BridgeLink>>,
) {
    use bevy::render::view::screenshot::{save_to_disk, Screenshot};
    if !keys.just_pressed(KeyCode::F9) {
        return;
    }
    let directory = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("captures")))
        .unwrap_or_else(|| PathBuf::from("captures"));
    if let Err(error) = std::fs::create_dir_all(&directory) {
        warn!("capture: can't make {}: {error}", directory.display());
        return;
    }
    let game = link.as_ref().and_then(|link| link.latest.as_ref());
    let level = game.and_then(|g| g.scenario_name.rsplit('\\').next()).filter(|l| !l.is_empty()).unwrap_or("viewer").to_string();
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
    let path = directory.join(format!("{level}_{stamp}.png"));
    info!(
        "capture: {} - {level} at tick {}, {}",
        path.display(),
        game.map_or(0, |g| g.game_tick),
        camera_argument(&camera)
    );
    commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
}

/// Frames the camera waits before it starts rendering. DLSS creates its
/// feature (on the camera's first frame) with GPU work wgpu does not track;
/// done while the window's surface is first configured, wgpu's wait for an
/// idle queue fails ("Failed to wait for GPU to come idle") and the app
/// stops. A moment later the surface has settled. (Adding DLSS to a camera
/// that already renders is not enough: it stays off.)
const CAMERA_START_FRAME: u32 = 30;

fn start_camera(mut frames: Local<u32>, mut camera: Single<&mut Camera, With<SolarikLighting>>) {
    *frames += 1;
    if *frames == CAMERA_START_FRAME {
        camera.is_active = true;
    }
}

/// Frames in one forward-and-back sweep of `--dolly`.
const DOLLY_PERIOD: u32 = 120;

/// Frames since the app started, as `--screenshot` and `--dolly` count them
/// (one count, so a capture's frame is the dolly's).
#[derive(Resource, Default)]
struct CaptureFrames(u32);

fn count_capture_frames(mut frames: ResMut<CaptureFrames>) {
    frames.0 += 1;
}

/// `HALO_RT_CACHE_LOG`: Solarik's world cache counters every 40 frames
/// (diagnostics for the interiors' slow drift).
fn log_world_cache(frames: Res<CaptureFrames>, diagnostics: Res<DiagnosticsStore>) {
    if frames.0 % 40 != 0 {
        return;
    }
    let mut line = format!("cache {}:", frames.0);
    for name in ["world_cache_active_cells_count", "world_cache_selected_cells", "world_cache_unlit_cells", "world_cache_selected_unlit", "world_cache_oldest_age", "world_cache_updated_cells"] {
        let path = DiagnosticPath::new(format!("render/solarik_lighting/{name}"));
        let value = diagnostics.get(&path).and_then(Diagnostic::value).unwrap_or(-1.0);
        line += &format!(" {}={value}", name.trim_start_matches("world_cache_"));
    }
    info!("{line}");
}

/// `--dolly`: sways the camera forward and back from where it started, and
/// with `--screenshot-every` stops there at `--screenshot-from` (a multiple
/// of the period: at rest), so the captures show the lighting settling
/// after the motion, from one view.
fn dolly_camera(
    args: Res<Args>,
    frames: Res<CaptureFrames>,
    mut start: Local<Option<Transform>>,
    mut camera: Single<&mut Transform, With<Camera3d>>,
) {
    let Some(reach) = args.dolly else { return };
    let start = *start.get_or_insert(**camera);
    // (HALO_RT_DOLLY_DURING: it sways on through the captures, taken at one
    // phase of each sweep: the lighting in motion, compared at one place)
    if args.screenshot_every.is_some() && frames.0 >= args.screenshot_from && std::env::var_os("HALO_RT_DOLLY_DURING").is_none() {
        camera.translation = start.translation;
        return;
    }
    let phase = (frames.0 % DOLLY_PERIOD) as f32 / DOLLY_PERIOD as f32 * std::f32::consts::TAU;
    camera.translation = start.translation + *start.forward() * reach * 0.5 * (1.0 - phase.cos());
}

#[cfg(feature = "dlss")]
fn toggle_dlss_rr(
    key_input: Res<ButtonInput<KeyCode>>,
    camera: Single<(Entity, Has<Dlss<DlssRayReconstructionFeature>>), With<SolarikLighting>>,
    dlss_rr_supported: Option<Res<DlssRayReconstructionSupported>>,
    mut commands: Commands,
) {
    if key_input.just_pressed(KeyCode::Digit3) && dlss_rr_supported.is_some() {
        let (entity, dlss) = *camera;
        if dlss {
            commands.entity(entity).remove::<(Dlss<DlssRayReconstructionFeature>, TemporalJitter, MipBias)>();
        } else {
            commands.entity(entity).insert(Dlss::<DlssRayReconstructionFeature>::default());
        }
    }
}

#[cfg(feature = "dlss")]
fn cycle_dlss_quality(key_input: Res<ButtonInput<KeyCode>>, mut dlss: Query<&mut Dlss<DlssRayReconstructionFeature>>) {
    if !key_input.just_pressed(KeyCode::Digit4) {
        return;
    }
    for mut dlss in &mut dlss {
        dlss.perf_quality_mode = match dlss.perf_quality_mode {
            DlssPerfQualityMode::Auto => DlssPerfQualityMode::Dlaa,
            DlssPerfQualityMode::Dlaa => DlssPerfQualityMode::Quality,
            DlssPerfQualityMode::Quality => DlssPerfQualityMode::Balanced,
            DlssPerfQualityMode::Balanced => DlssPerfQualityMode::Performance,
            DlssPerfQualityMode::Performance => DlssPerfQualityMode::UltraPerformance,
            DlssPerfQualityMode::UltraPerformance => DlssPerfQualityMode::Auto,
        };
    }
}

/// `--screenshot`: capture after the world cache and denoiser converge.
fn screenshot_and_exit(
    mut commands: Commands,
    args: Res<Args>,
    frames: Res<CaptureFrames>,
    mut exit: MessageWriter<AppExit>,
) {
    use bevy::render::view::screenshot::{save_to_disk, Screenshot};
    let frames = &frames.0;
    if let (Some(every), Some(path)) = (args.screenshot_every, &args.screenshot)
        && every > 0
        && *frames % every == 0
        && *frames >= args.screenshot_from
        && *frames < args.screenshot_after
    {
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let numbered = path.with_file_name(format!("{stem}_{:06}.png", *frames));
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(numbered));
    }
    if *frames == args.screenshot_after
        && let Some(path) = args.screenshot.clone()
    {
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
    }
    if *frames == args.screenshot_after + 30 {
        exit.write(AppExit::Success);
    }
}

#[derive(Component)]
struct ControlText;

#[derive(Component)]
struct PerformanceText;

fn setup_ui(mut commands: Commands) {
    commands.spawn((
        ControlText,
        options::StatsText,
        Text::default(),
        TextFont { font_size: FontSize::Px(12.0), ..default() },
        Node {
            position_type: PositionType::Absolute,
            bottom: px(8.0),
            left: px(8.0),
            padding: px(4.0).all(),
            border_radius: BorderRadius::all(px(4.0)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.10, 0.10, 0.10, 0.75)),
    ));
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            right: px(0.0),
            padding: px(4.0).all(),
            border_radius: BorderRadius::bottom_left(px(4.0)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.10, 0.10, 0.10, 0.8)),
        options::StatsText,
        children![(PerformanceText, Text::default(), TextFont { font_size: FontSize::Px(10.0), ..default() })],
    ));
}

fn update_control_text(
    mut text: Single<&mut Text, With<ControlText>>,
    settings: Res<LightingSettings>,
    stats: Option<Res<LevelStats>>,
    link: Option<ResMut<bridge::BridgeLink>>,
    bridged: Option<Res<bridge::BridgeObjects>>,
    follow: Option<Res<bridge::FollowGame>>,
    forward: Option<Res<bridge::ForwardInput>>,
    overlay: Option<Res<overlay::OverlayStats>>,
    time: Res<Time>,
    window: Option<Single<&Window, With<bevy::window::PrimaryWindow>>>,
    // (seconds counted, the game's frames per second, frames drawn since the
    // count began, frames drawn per second)
    mut rate: Local<(f32, u32, u32, u32)>,
    #[cfg(feature = "dlss")] dlss_rr_supported: Option<Res<DlssRayReconstructionSupported>>,
    #[cfg(feature = "dlss")] dlss_camera: Query<Option<&Dlss<DlssRayReconstructionFeature>>, With<SolarikLighting>>,
) {
    let mut s = String::new();
    if let (Some(mut link), Some(bridged), Some(follow)) = (link, bridged, follow) {
        // the game's frame rate and this one's, over the last second
        rate.0 += time.delta_secs();
        rate.2 += 1;
        if rate.0 >= 1.0 {
            rate.1 = (link.frames as f32 / rate.0).round() as u32;
            rate.3 = (rate.2 as f32 / rate.0).round() as u32;
            link.frames = 0;
            rate.2 = 0;
            rate.0 = 0.0;
        }
        let game = link.latest.as_ref();
        let unfocused = window.is_some_and(|window| !window.focused);
        s += &format!(
            "OpenCE bridge: {} at tick {}, {} game frames/s, {} drawn/s{}, drawing {} of {} objects - F8: {}{}\n",
            game.map_or("", |g| g.scenario_name.as_str()),
            game.map_or(0, |g| g.game_tick),
            rate.1,
            rate.3,
            if unfocused { " (window unfocused)" } else { "" },
            bridged.count(),
            game.map_or(0, |g| g.objects.len()),
            if follow.0 { "free camera" } else { "follow the game's camera" },
            if forward.is_some_and(|f| f.active) {
                " - playing (F12 lets go of the mouse)"
            } else if follow.0 {
                " - click to play"
            } else {
                ""
            }
        );
        if let Some(overlay) = overlay {
            s += &format!(
                "the game's overlay (HUD, effects, transparent surfaces): {}x{}\n",
                overlay.size.0, overlay.size.1
            );
        }
    }
    if let Some(stats) = stats {
        s += &format!(
            "{} - {} BSPs ({}k tris), {} objects ({}k tris), {} emissive meshes\n",
            stats.map_name,
            stats.bsps,
            stats.triangles / 1000,
            stats.objects,
            stats.object_triangles / 1000,
            stats.emissive_meshes
        );
    }
    s += &format!("[ ]: emissive {:.0}   - =: sun {:.0}   9 0: sky {:.0}   , .: exposure EV {:.1}\n",
        settings.emissive_scale, settings.sun_scale, settings.sky_scale, settings.exposure_ev100);

    #[cfg(feature = "dlss")]
    if dlss_rr_supported.is_some() {
        if let Ok(Some(dlss)) = dlss_camera.single() {
            s += &format!("\n3: disable DLSS Ray Reconstruction   4: quality {:?}", dlss.perf_quality_mode);
        } else {
            s += "\n3: enable DLSS Ray Reconstruction";
        }
    } else {
        s += "\nDLSS Ray Reconstruction not supported on this GPU";
    }
    #[cfg(not(feature = "dlss"))]
    {
        s += "\nNo denoiser: build with --features dlss";
    }
    s += "\nWASD/QE fly, Shift run, right mouse or M to look, P print camera, F9 capture, F10 options";
    text.0 = s;
}

fn update_performance_text(
    mut text: Single<&mut Text, With<PerformanceText>>,
    diagnostics: Res<DiagnosticsStore>,
    #[cfg(feature = "dlss")] dlss_camera: Query<Has<Dlss<DlssRayReconstructionFeature>>, With<SolarikLighting>>,
) {
    text.0.clear();
    let mut total = 0.0;
    let mut add = |name: &str, path: &'static str| {
        if let Some(value) = diagnostics.get(&DiagnosticPath::new(path)).and_then(Diagnostic::smoothed) {
            text.push_str(&format!("{name:17}  {value:.2} ms\n"));
            total += value;
        }
    };
    add("Sky distribution", "render/solarik_lighting/sky_distribution/elapsed_gpu");
    add("Light tiles", "render/solarik_lighting/presample_light_tiles/elapsed_gpu");
    add("World cache", "render/solarik_lighting/world_cache/elapsed_gpu");
    add("Direct", "render/solarik_lighting/direct_lighting/elapsed_gpu");
    add("Diffuse indirect", "render/solarik_lighting/diffuse_indirect_lighting/elapsed_gpu");
    add("Specular indirect", "render/solarik_lighting/specular_indirect_lighting/elapsed_gpu");
    #[cfg(feature = "dlss")]
    if matches!(dlss_camera.single(), Ok(true)) {
        add("DLSS-RR", "render/dlss_ray_reconstruction/elapsed_gpu");
    }
    text.push_str(&format!("{:17}  {total:.2} ms", "Total"));
}
