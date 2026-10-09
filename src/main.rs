//! Halo: Combat Evolved levels rendered by Bevy Solari (real-time ray
//! tracing), with DLSS Ray Reconstruction when built with `--features dlss`.
//!
//! Phases 1-3 of ROADMAP.md: a retail Xbox map's structure BSPs, placed
//! objects and sky, with Halo's materials, lit by the level's sun and sky and
//! by its light-emitting surfaces.

#![allow(clippy::too_many_arguments, reason = "Bevy systems take their resources and queries as arguments")]

mod level;
mod material;
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
    solari::{
        pathtracer::{Pathtracer, PathtracingPlugin},
        prelude::{SolariLighting, SolariPlugins},
    },
};
use halo_map::{
    math::{halo_to_bevy, METERS_PER_WORLD_UNIT},
    scenario, CacheFile,
};
use level::{EmissiveSurface, LevelAssets, LevelBuilder, LevelStats};
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
    /// map name (b30, bloodgulch, ...) or path to a .map file
    #[argh(positional, default = "String::from(\"b30\")")]
    map: String,
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
    /// The map file: the argument itself if it is a path, else `<name>.map`
    /// in the first maps folder that has it (see [`Args::map_dirs`]).
    fn map_path(&self) -> Result<PathBuf, String> {
        let direct = PathBuf::from(&self.map);
        if direct.extension().is_some() || direct.is_file() {
            return Ok(direct);
        }
        let file = format!("{}.map", self.map);
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
#[derive(Resource, Clone)]
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
    let args: Args = argh::from_env();
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
    let level_sky = sky::SkyLighting::from_map(&cache);

    let mut app = App::new();

    #[cfg(feature = "dlss")]
    app.insert_resource(DlssProjectId(bevy::asset::uuid::uuid!("3f1d6a2e-8a4b-4f5e-9c3d-1b2a7e6f0d91")));

    app.add_plugins((default_plugins(&args), SolariPlugins, FreeCameraPlugin, RenderDiagnosticsPlugin, HaloMaterialsPlugin))
        .insert_resource(args.clone())
        .insert_resource(LevelSky(level_sky))
        .insert_resource(LightingSettings::default())
        .insert_resource(MapFile(Some(cache)))
        .add_systems(Startup, (setup_level, setup_camera, setup_ui).chain())
        .add_systems(PostUpdate, level::follow_camera.before(TransformSystems::Propagate));

    if args.screenshot.is_some() {
        app.add_systems(Update, screenshot_and_exit);
    }

    if args.pathtracer {
        app.add_plugins(PathtracingPlugin);
    } else {
        app.add_systems(Update, ((adjust_lighting, apply_lighting).chain(), print_camera))
            .add_systems(PostUpdate, (update_control_text, update_performance_text));
        #[cfg(feature = "dlss")]
        app.add_systems(Update, (toggle_dlss_rr, cycle_dlss_quality));
    }

    app.run()
}

/// The open cache file, consumed by `setup_level`.
#[derive(Resource)]
struct MapFile(Option<CacheFile>);

fn default_plugins(args: &Args) -> impl PluginGroup {
    use bevy::render::{
        settings::{Backends, WgpuSettings},
        RenderPlugin,
    };
    let title = format!("Halo RT - {}", args.map);
    DefaultPlugins
        .set(WindowPlugin { primary_window: Some(Window { title, ..default() }), ..default() })
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

fn setup_level(
    mut commands: Commands,
    mut map: ResMut<MapFile>,
    args: Res<Args>,
    render_device: Res<RenderDevice>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut environment_materials: ResMut<Assets<HaloEnvironmentMaterial>>,
    mut transparent_materials: ResMut<Assets<HaloTransparentMaterial>>,
    mut images: ResMut<Assets<Image>>,
    neutral_detail: Res<NeutralDetail>,
) {
    let cache = map.0.take().expect("level loads once");
    let mut assets = LevelAssets {
        meshes: &mut meshes,
        materials: &mut materials,
        environment_materials: &mut environment_materials,
        transparent_materials: &mut transparent_materials,
        images: &mut images,
        neutral_detail: neutral_detail.0.clone(),
        flat_normal: Handle::default(),
    };
    assets.flat_normal = assets.images.add(material::flat_normal_image());
    let bc_supported = render_device.features().contains(WgpuFeatures::TEXTURE_COMPRESSION_BC);
    let start = std::time::Instant::now();
    let mut builder = LevelBuilder::new(&cache, bc_supported);
    builder.fixture_lights = !args.no_fixture_lights;
    builder.show_light_proxies = args.show_light_proxies;
    builder.matte = args.matte;
    if let Err(e) = builder.spawn_bsps(args.bsp, !args.pathtracer, &mut commands, &mut assets) {
        error!("loading {}: {e}", cache.header.name);
    }
    if !args.no_objects
        && let Err(e) = builder.spawn_objects(!args.pathtracer, &mut commands, &mut assets)
    {
        error!("placing objects: {e}");
    }
    if let Some(model) = halo_map::sky::skies(&cache).ok().and_then(|s| s.into_iter().find_map(|s| s.model)).and_then(|m| m.datum)
        && let Err(e) = builder.spawn_sky(model, &mut commands, &mut assets)
    {
        error!("sky: {e}");
    }
    let stats = builder.stats;
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
}

#[derive(Resource)]
struct CameraStart(Transform);

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
    mut images: ResMut<Assets<Image>>,
    #[cfg(feature = "dlss")] dlss_rr_supported: Option<Res<DlssRayReconstructionSupported>>,
) {
    let mut camera = commands.spawn((
        Camera3d::default(),
        Camera { clear_color: ClearColorConfig::Custom(Color::BLACK), ..default() },
        FreeCamera { walk_speed: 6.0, run_speed: 25.0, ..default() },
        start.0,
        // Msaa::Off and STORAGE_BINDING main textures are required by Solari
        CameraMainTextureUsages::default().with(TextureUsages::STORAGE_BINDING),
        Msaa::Off,
        Exposure { ev100: settings.exposure_ev100 },
    ));
    if args.pathtracer {
        camera.insert(Pathtracer::default());
    } else {
        camera.insert(SolariLighting::default());
    }

    if let Some((color, power)) = level_sky.0.sky {
        let cubemap = sky::gradient_cubemap(color, &mut images);
        camera.insert((
            EnvironmentMapLight {
                diffuse_map: cubemap.clone(),
                specular_map: cubemap.clone(),
                intensity: power * settings.sky_scale,
                ..default()
            },
            Skybox { image: Some(cubemap), brightness: power * settings.sky_scale, ..default() },
        ));
    }

    #[cfg(feature = "dlss")]
    if dlss_rr_supported.is_some() {
        camera.insert(Dlss::<DlssRayReconstructionFeature>::default());
    }

    if let Some((direction, color, power)) = level_sky.0.sun {
        commands.spawn((
            DirectionalLight {
                color,
                illuminance: power * settings.sun_scale,
                // Solari replaces shadow maps
                shadow_maps_enabled: false,
                ..default()
            },
            Transform::default().looking_to(-direction, Vec3::Y),
        ));
    }
}

/// `[`/`]` emissive, `-`/`=` sun, `9`/`0` sky, `,`/`.` exposure, R ReSTIR.
fn adjust_lighting(
    keys: Res<ButtonInput<KeyCode>>,
    mut settings: ResMut<LightingSettings>,
    mut solari: Query<&mut SolariLighting>,
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
    if keys.just_pressed(KeyCode::KeyR) {
        for mut solari in &mut solari {
            solari.restir = !solari.restir;
        }
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
) {
    if !settings.is_changed() {
        return;
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

#[cfg(feature = "dlss")]
fn toggle_dlss_rr(
    key_input: Res<ButtonInput<KeyCode>>,
    camera: Single<(Entity, Has<Dlss<DlssRayReconstructionFeature>>), With<SolariLighting>>,
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
    const CAPTURE_FRAME: u32 = 300;
    *frames += 1;
    if *frames == CAPTURE_FRAME
        && let Some(path) = args.screenshot.clone()
    {
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
    }
    if *frames == CAPTURE_FRAME + 30 {
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
        children![(PerformanceText, Text::default(), TextFont { font_size: FontSize::Px(10.0), ..default() })],
    ));
}

fn update_control_text(
    mut text: Single<&mut Text, With<ControlText>>,
    settings: Res<LightingSettings>,
    stats: Option<Res<LevelStats>>,
    solari: Single<&SolariLighting>,
    #[cfg(feature = "dlss")] dlss_rr_supported: Option<Res<DlssRayReconstructionSupported>>,
    #[cfg(feature = "dlss")] dlss_camera: Query<Option<&Dlss<DlssRayReconstructionFeature>>, With<SolariLighting>>,
) {
    let mut s = String::new();
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
    s += if solari.restir { "R: disable ReSTIR" } else { "R: enable ReSTIR" };

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
    s += "\nWASD/QE fly, Shift run, right mouse or M to look, P print camera";
    text.0 = s;
}

fn update_performance_text(
    mut text: Single<&mut Text, With<PerformanceText>>,
    diagnostics: Res<DiagnosticsStore>,
    #[cfg(feature = "dlss")] dlss_camera: Query<Has<Dlss<DlssRayReconstructionFeature>>, With<SolariLighting>>,
) {
    text.0.clear();
    let mut total = 0.0;
    let mut add = |name: &str, path: &'static str| {
        if let Some(value) = diagnostics.get(&DiagnosticPath::new(path)).and_then(Diagnostic::smoothed) {
            text.push_str(&format!("{name:17}  {value:.2} ms\n"));
            total += value;
        }
    };
    add("Light tiles", "render/solari_lighting/presample_light_tiles/elapsed_gpu");
    add("World cache", "render/solari_lighting/world_cache/elapsed_gpu");
    add("Lighting", "render/solari_lighting/lighting/elapsed_gpu");
    #[cfg(feature = "dlss")]
    if matches!(dlss_camera.single(), Ok(true)) {
        add("DLSS-RR", "render/dlss_ray_reconstruction/elapsed_gpu");
    }
    text.push_str(&format!("{:17}  {total:.2} ms", "Total"));
}
