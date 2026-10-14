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
mod level;
mod lights;
mod material;
mod options;
mod overlay;
mod sky;
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
    /// (press P in the viewer to print the current view in this form)
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
            bridge::launch_game(&exe, &map_path, !args.menu, &bridge_name, args.show_game, !args.no_overlay, simulation)
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

    app.add_plugins((default_plugins(&args, overlay), SolarikPlugins, FreeCameraPlugin, RenderDiagnosticsPlugin, HaloMaterialsPlugin))
        .insert_resource(args.clone())
        .insert_resource(overlay::OverlayMode(overlay))
        .insert_resource(saved.as_ref().map_or_else(LightingSettings::default, |s| s.lighting.clone()))
        .insert_resource(MapFile(Some(cache)))
        .add_systems(Startup, (setup_level, setup_camera, setup_ui).chain())
        .add_systems(Update, (start_camera, sync_sky_light))
        .add_systems(PostUpdate, level::follow_camera.before(TransformSystems::Propagate))
        .add_systems(PostUpdate, level::sync_traced_visibility.after(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate));

    if args.screenshot.is_some() {
        app.add_systems(Update, screenshot_and_exit);
    }
    if let Some(mut link) = link {
        link.sync_to_overlay = overlay;
        app.insert_resource(link)
            .add_plugins((bridge::BridgePlugin, lights::DynamicLightsPlugin))
            .add_systems(PreUpdate, follow_level);
        if overlay {
            app.add_plugins(overlay::OverlayPlugin);
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
                (adjust_lighting.run_if(bridge::not_forwarding), apply_lighting).chain(),
                print_camera.run_if(bridge::not_forwarding),
            ),
        )
            .add_systems(PostUpdate, (update_control_text, update_performance_text));
        #[cfg(feature = "dlss")]
        app.add_systems(Update, (toggle_dlss_rr, cycle_dlss_quality).run_if(bridge::not_forwarding));
    }

    app.run()
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
    let mut window = Window { title, ..default() };
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
    images: ResMut<'w, Assets<Image>>,
    inverse_bindposes: ResMut<'w, Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>,
    neutral_detail: Res<'w, NeutralDetail>,
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
    let mut assets = LevelAssets {
        meshes: &mut params.meshes,
        materials: &mut params.materials,
        environment_materials: &mut params.environment_materials,
        transparent_materials: &mut params.transparent_materials,
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
    builder.fixture_lights = !args.no_fixture_lights;
    builder.show_light_proxies = args.show_light_proxies;
    builder.matte = args.matte;
    // (the game's overlay has them)
    builder.skip_transparent = overlay;
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
            Ok(Some(cubemap)) => commands.insert_resource(SkyCubemap(cubemap)),
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

/// The sky model rendered into a cubemap, for the environment light.
#[derive(Resource)]
struct SkyCubemap(Handle<Image>);

fn bsp_overview(cache: &CacheFile, index: usize) -> Option<Transform> {
    let reference = halo_map::bsp::references(cache).ok()?.into_iter().find(|r| r.index == index)?;
    let [x, y, z] = halo_map::bsp::Bsp::load(cache, &reference).ok()?.world_bounds().ok()?;
    let centre = [(x[0] + x[1]) / 2.0, (y[0] + y[1]) / 2.0, (z[0] + z[1]) / 2.0];
    Some(Transform::from_translation(Vec3::from(halo_to_bevy(centre, METERS_PER_WORLD_UNIT))))
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
        Camera3d::default(),
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
        camera.insert(SolarikLighting::default());
    }
    // a bridged camera follows the game's until F detaches it
    if !args.bridge {
        camera.insert(FreeCamera { walk_speed: 6.0, run_speed: 25.0, ..default() });
    }

    if let Some(sky) = sky_environment(&level_sky, sky_cubemap.as_deref(), &settings, &mut images) {
        camera.insert(sky);
    }

    #[cfg(feature = "dlss")]
    if dlss_rr_supported.is_some() {
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
    let cubemap = match sky_cubemap {
        Some(sky) => sky.0.clone(),
        None => sky::gradient_cubemap(color, images),
    };
    Some((
        EnvironmentMapLight {
            diffuse_map: cubemap.clone(),
            specular_map: cubemap.clone(),
            intensity: power * settings.sky_scale,
            ..default()
        },
        Skybox { image: Some(cubemap), brightness: power * settings.sky_scale, ..default() },
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
        let cubemap = world.get_resource::<SkyCubemap>().map(|c| SkyCubemap(c.0.clone()));
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

/// P: log the camera as a `--camera` argument.
fn print_camera(keys: Res<ButtonInput<KeyCode>>, camera: Single<&Transform, With<Camera3d>>) {
    if !keys.just_pressed(KeyCode::KeyP) {
        return;
    }
    let p = camera.translation / METERS_PER_WORLD_UNIT;
    let f = camera.forward();
    // Bevy (x, y, z) is Halo (x, -z, y)
    let (hx, hy, hz) = (f.x, -f.z, f.y);
    let yaw = hy.atan2(hx).to_degrees();
    let pitch = hz.clamp(-1.0, 1.0).asin().to_degrees();
    info!("--camera {:.2},{:.2},{:.2},{yaw:.1},{pitch:.1}", p.x, -p.z, p.y);
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
    mut frames: Local<u32>,
    mut exit: MessageWriter<AppExit>,
) {
    use bevy::render::view::screenshot::{save_to_disk, Screenshot};
    *frames += 1;
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
    mut rate: Local<(f32, u32)>,
    #[cfg(feature = "dlss")] dlss_rr_supported: Option<Res<DlssRayReconstructionSupported>>,
    #[cfg(feature = "dlss")] dlss_camera: Query<Option<&Dlss<DlssRayReconstructionFeature>>, With<SolarikLighting>>,
) {
    let mut s = String::new();
    if let (Some(mut link), Some(bridged), Some(follow)) = (link, bridged, follow) {
        // the game's frame rate, over the last second
        rate.0 += time.delta_secs();
        if rate.0 >= 1.0 {
            rate.1 = (link.frames as f32 / rate.0).round() as u32;
            link.frames = 0;
            rate.0 = 0.0;
        }
        let game = link.latest.as_ref();
        s += &format!(
            "OpenCE bridge: {} at tick {}, {} game frames/s, drawing {} of {} objects - F8: {}{}\n",
            game.map_or("", |g| g.scenario_name.as_str()),
            game.map_or(0, |g| g.game_tick),
            rate.1,
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
    s += "\nWASD/QE fly, Shift run, right mouse or M to look, P print camera, F10 options";
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
