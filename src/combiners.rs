//! The transparent surfaces' combiner parameters in one storage buffer, which
//! the materials index by a slot ([`HaloCombinerSlot`], the material's data
//! binding), rather than each material's own uniform. The level's animated
//! surfaces change every frame (the library's 600 strips and glows), and as
//! a material's own binding each change went through the asset pipeline:
//! extracted, prepared, its bind group built, its data uploaded, 1 to 1.5 ms
//! a frame in c20's fights and what the exclusive BLAS compaction waited for.
//! Here the frame's changes are written into the table, and its one buffer
//! is rewritten in place.

use crate::material::{HaloCombinerParams, HaloTransparentMaterial};
use bevy::{
    asset::RenderAssetUsages,
    platform::collections::HashMap,
    prelude::*,
    render::{
        render_resource::{encase, ShaderType},
        storage::ShaderBuffer,
    },
};

/// A material not yet given a slot ([`register_slots`] gives it one, the
/// frame it appears).
pub const NO_SLOT: u32 = u32::MAX;

/// Slots the table starts with; it doubles when full (a growth remakes the
/// buffer, and the renderer rebinds every material that reads it).
const FIRST_CAPACITY: usize = 1024;

pub struct CombinersPlugin;

impl Plugin for CombinersPlugin {
    fn build(&self, app: &mut App) {
        let table = CombinerTable::new(&mut app.world_mut().resource_mut::<Assets<ShaderBuffer>>());
        app.insert_resource(table)
            .add_systems(Update, (register_slots, crate::level::animate_transparent, flush).chain());
    }
}

static COMBINERS: std::sync::OnceLock<Handle<ShaderBuffer>> = std::sync::OnceLock::new();

/// The table's buffer, every transparent material's `combiners`.
pub fn combiners() -> Handle<ShaderBuffer> {
    COMBINERS.get().cloned().unwrap_or_default()
}

/// The table: each slot's params as the shader reads them (`data`, the
/// buffer's bytes) and as last written (`params`, to write only changes).
#[derive(Resource)]
pub struct CombinerTable {
    handle: Handle<ShaderBuffer>,
    stride: usize,
    data: Vec<u8>,
    params: Vec<Option<HaloCombinerParams>>,
    free: Vec<u32>,
    by_material: HashMap<AssetId<HaloTransparentMaterial>, u32>,
    dirty: bool,
}

impl CombinerTable {
    fn new(buffers: &mut Assets<ShaderBuffer>) -> Self {
        let stride = HaloCombinerParams::min_size().get() as usize;
        let data = vec![0u8; stride * FIRST_CAPACITY];
        let handle = buffers.add(ShaderBuffer::new(data.clone(), RenderAssetUsages::default()));
        let _ = COMBINERS.set(handle.clone());
        Self {
            handle,
            stride,
            data,
            params: Vec::new(),
            free: Vec::new(),
            by_material: HashMap::default(),
            dirty: false,
        }
    }

    fn capacity(&self) -> usize {
        self.data.len() / self.stride
    }

    /// A slot's params as last written.
    pub fn params(&self, slot: u32) -> Option<&HaloCombinerParams> {
        self.params.get(slot as usize)?.as_ref()
    }

    fn alloc(&mut self) -> u32 {
        if let Some(slot) = self.free.pop() {
            return slot;
        }
        let slot = self.params.len();
        if slot >= self.capacity() {
            let capacity = self.capacity() * 2;
            self.data.resize(capacity * self.stride, 0);
            self.dirty = true;
        }
        self.params.push(None);
        slot as u32
    }

    fn free(&mut self, slot: u32) {
        if let Some(params) = self.params.get_mut(slot as usize) {
            *params = None;
            self.free.push(slot);
        }
    }

    /// Writes a slot's params, when they differ from the last written.
    pub fn set(&mut self, slot: u32, params: &HaloCombinerParams) {
        let index = slot as usize;
        let Some(current) = self.params.get_mut(index) else { return };
        if current.as_ref() == Some(params) {
            return;
        }
        let mut buffer = encase::StorageBuffer::new(Vec::<u8>::new());
        buffer.write(params).expect("combiner params");
        let bytes = buffer.into_inner();
        let start = index * self.stride;
        self.data[start..start + bytes.len()].copy_from_slice(&bytes);
        *current = Some(params.clone());
        self.dirty = true;
    }
}

/// Each transparent material its slot: a new one, or a copy of another
/// (which carries its original's slot), is given one and its params written;
/// a removed one's slot is let go.
pub fn register_slots(
    mut table: ResMut<CombinerTable>,
    mut materials: ResMut<Assets<HaloTransparentMaterial>>,
    mut events: MessageReader<AssetEvent<HaloTransparentMaterial>>,
) {
    for event in events.read() {
        if let AssetEvent::Removed { id } = event
            && let Some(slot) = table.by_material.remove(id)
        {
            table.free(slot);
        }
    }
    let unregistered: Vec<_> = materials
        .iter()
        .filter(|(id, material)| table.by_material.get(id) != Some(&material.slot))
        .map(|(id, _)| id)
        .collect();
    for id in unregistered {
        let slot = table.alloc();
        table.by_material.insert(id, slot);
        if let Some(mut material) = materials.get_mut(id) {
            material.slot = slot;
            let params = material.params.clone();
            table.set(slot, &params);
        }
    }
}

/// The frame's changes to the buffer (the same size: rewritten in place).
pub fn flush(mut table: ResMut<CombinerTable>, mut buffers: ResMut<Assets<ShaderBuffer>>) {
    if !table.dirty {
        return;
    }
    table.dirty = false;
    if let Some(mut buffer) = buffers.get_mut(&table.handle) {
        *buffer = ShaderBuffer::new(table.data.clone(), RenderAssetUsages::default());
    }
}
