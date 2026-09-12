#![allow(clippy::type_complexity)]

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::Arc;
use std::thread;
use std::time::SystemTime;

use chunkedge::prelude::*;
use chunkedge::spawn::IsFlat;
use chunkedge_server::nbt::Value;
use chunkedge_server::protocol::WritePacket as _;
use chunkedge_server::protocol::packets::play::set_time_s2c::{ClockState, SetTimeS2c};
use chunkedge_server::protocol::{VarInt, VarLong};
use flume::{Receiver, Sender};
use noise::{NoiseFn, SuperSimplex};
use tracing::info;

const SPAWN_POS: DVec3 = DVec3::new(0.0, 200.0, 0.0);
const HEIGHT: u32 = 384;

struct ChunkWorkerState {
    sender: Sender<(ChunkPos, UnloadedChunk)>,
    receiver: Receiver<ChunkPos>,
    seed: u32,
    // Noise functions
    density: SuperSimplex,
    hilly: SuperSimplex,
    stone: SuperSimplex,
    gravel: SuperSimplex,
    grass: SuperSimplex,
    tree: SuperSimplex,
}

#[derive(Resource)]
struct GameState {
    /// Chunks that need to be generated. Chunks without a priority have already
    /// been sent to the thread pool.
    pending: HashMap<ChunkPos, Option<Priority>>,
    sender: Sender<ChunkPos>,
    receiver: Receiver<(ChunkPos, UnloadedChunk)>,
}

/// The order in which chunks should be processed by the thread pool. Smaller
/// values are sent first.
type Priority = u64;

pub fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                (
                    init_clients,
                    remove_unviewed_chunks,
                    update_client_views,
                    send_recv_chunks,
                )
                    .chain(),
                despawn_disconnected_clients,
            ),
        )
        .run();
}

fn setup(
    mut commands: Commands,
    server: Res<Server>,
    mut dimensions: ResMut<DimensionTypeRegistry>,
    biomes: Res<BiomeRegistry>,
) {
    let seconds_per_day = 86_400;
    let seed = (SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        / seconds_per_day) as u32;

    info!("current seed: {seed}");

    let (finished_sender, finished_receiver) = flume::unbounded();
    let (pending_sender, pending_receiver) = flume::unbounded();

    let state = Arc::new(ChunkWorkerState {
        sender: finished_sender,
        receiver: pending_receiver,
        seed,
        density: SuperSimplex::new(seed),
        hilly: SuperSimplex::new(seed.wrapping_add(1)),
        stone: SuperSimplex::new(seed.wrapping_add(2)),
        gravel: SuperSimplex::new(seed.wrapping_add(3)),
        grass: SuperSimplex::new(seed.wrapping_add(4)),
        tree: SuperSimplex::new(seed.wrapping_add(5)),
    });

    // Chunks are generated in a thread pool for parallelism and to avoid blocking
    // the main tick loop. You can use your thread pool of choice here (rayon,
    // bevy_tasks, etc). Only the standard library is used in the example for the
    // sake of simplicity.
    //
    // If your chunk generation algorithm is inexpensive then there's no need to do
    // this.
    for _ in 0..thread::available_parallelism().unwrap().get() {
        let state = state.clone();
        thread::spawn(move || chunk_worker(state));
    }

    commands.insert_resource(GameState {
        pending: HashMap::new(),
        sender: pending_sender,
        receiver: finished_receiver,
    });

    // ChunkEdge has no lighting engine, so sky light arrives as zero
    // everywhere. On 26.x the client renders that as
    // (dark ambient `#0a0a0a` + zero sky/block) which reads as ~black.
    // Fake fullbright for this demo: push the visual ambient/sky
    // attributes to white and freeze the clock at noon. Library stays
    // light-agnostic; this is purely example-side.
    for (_, _, dim) in dimensions.iter_mut() {
        dim.ambient_light = 1.0;
        let attrs = dim.attributes.get_or_insert_with(Compound::new);
        attrs.insert(
            "minecraft:visual/ambient_light_color",
            Value::String("#ffffff".to_owned()),
        );
        attrs.insert(
            "minecraft:visual/sky_light_color",
            Value::String("#ffffff".to_owned()),
        );
        attrs.insert("minecraft:visual/sky_light_factor", Value::Float(1.0));
    }

    let layer = LayerBundle::new(ident!("overworld"), &dimensions, &biomes, &server);

    commands.spawn(layer);
}

fn init_clients(
    mut clients: Query<
        (
            &mut Client,
            &mut EntityLayerId,
            &mut VisibleChunkLayer,
            &mut VisibleEntityLayers,
            &mut Position,
            &mut GameMode,
            &mut IsFlat,
        ),
        Added<Client>,
    >,
    layers: Query<Entity, (With<ChunkLayer>, With<EntityLayer>)>,
) {
    for (
        mut client,
        mut layer_id,
        mut visible_chunk_layer,
        mut visible_entity_layers,
        mut pos,
        mut game_mode,
        mut is_flat,
    ) in &mut clients
    {
        let layer = layers.single().unwrap();

        layer_id.0 = layer;
        visible_chunk_layer.0 = layer;
        visible_entity_layers.0.insert(layer);
        pos.set(SPAWN_POS);
        *game_mode = GameMode::Creative;
        is_flat.0 = true;

        // Freeze the world clock at noon so the demo never drifts into
        // night while sky light is faked. One shot is enough with rate 0.
        client.write_packet(&SetTimeS2c {
            world_age: 0,
            clocks: vec![ClockState {
                // World-clock registry ID: `minecraft:overworld` is 0 in
                // the vanilla codec.
                clock: VarInt(0),
                total_ticks: VarLong(6000),
                partial_tick: 0.0,
                rate: 0.0,
            }],
        });
    }
}

fn remove_unviewed_chunks(mut layers: Query<&mut ChunkLayer>) {
    layers
        .single_mut()
        .unwrap()
        .retain_chunks(|_, chunk| chunk.viewer_count_mut() > 0);
}

fn update_client_views(
    mut layers: Query<&mut ChunkLayer>,
    mut clients: Query<(&mut Client, View, OldView)>,
    mut state: ResMut<GameState>,
) {
    let layer = layers.single_mut().unwrap();

    for (client, view, old_view) in &mut clients {
        let view = view.get();
        let queue_pos = |pos: ChunkPos| {
            if layer.chunk(pos).is_none() {
                match state.pending.entry(pos) {
                    Entry::Occupied(mut oe) => {
                        if let Some(priority) = oe.get_mut() {
                            let dist = view.pos.distance_squared(pos);
                            *priority = (*priority).min(dist);
                        }
                    }
                    Entry::Vacant(ve) => {
                        let dist = view.pos.distance_squared(pos);
                        ve.insert(Some(dist));
                    }
                }
            }
        };

        // Queue all the new chunks in the view to be sent to the thread pool.
        if client.is_added() {
            view.iter().for_each(queue_pos);
        } else {
            let old_view = old_view.get();
            if old_view != view {
                view.diff(old_view).for_each(queue_pos);
            }
        }
    }
}

fn send_recv_chunks(mut layers: Query<&mut ChunkLayer>, state: ResMut<GameState>) {
    let mut layer = layers.single_mut().unwrap();
    let state = state.into_inner();

    // Insert the chunks that are finished generating into the instance.
    for (pos, chunk) in state.receiver.drain() {
        layer.insert_chunk(pos, chunk);
        assert!(state.pending.remove(&pos).is_some());
    }

    // Collect all the new chunks that need to be loaded this tick.
    let mut to_send = vec![];

    for (pos, priority) in &mut state.pending {
        if let Some(pri) = priority.take() {
            to_send.push((pri, pos));
        }
    }

    // Sort chunks by ascending priority.
    to_send.sort_unstable_by_key(|(pri, _)| *pri);

    // Send the sorted chunks to be loaded.
    for (_, pos) in to_send {
        let _ = state.sender.try_send(*pos);
    }
}

fn chunk_worker(state: Arc<ChunkWorkerState>) {
    while let Ok(pos) = state.receiver.recv() {
        let mut chunk = UnloadedChunk::with_height(HEIGHT);

        for offset_z in 0..16 {
            for offset_x in 0..16 {
                let x = offset_x as i32 + pos.x * 16;
                let z = offset_z as i32 + pos.z * 16;

                let mut in_terrain = false;
                let mut depth = 0;

                // Fill in the terrain column.
                for y in (0..chunk.height() as i32).rev() {
                    const WATER_HEIGHT: i32 = 55;

                    let p = DVec3::new(f64::from(x), f64::from(y), f64::from(z));

                    let block = if has_terrain_at(&state, p) {
                        let gravel_height = WATER_HEIGHT
                            - 1
                            - (fbm(&state.gravel, p / 10.0, 3, 2.0, 0.5) * 6.0).floor() as i32;

                        if in_terrain {
                            if depth > 0 {
                                depth -= 1;
                                if y < gravel_height {
                                    BlockState::GRAVEL
                                } else {
                                    BlockState::DIRT
                                }
                            } else {
                                BlockState::STONE
                            }
                        } else {
                            in_terrain = true;
                            let n = noise01(&state.stone, p / 15.0);

                            depth = (n * 5.0).round() as u32;

                            if y < gravel_height {
                                BlockState::GRAVEL
                            } else if y < WATER_HEIGHT - 1 {
                                BlockState::DIRT
                            } else {
                                BlockState::GRASS_BLOCK
                            }
                        }
                    } else {
                        in_terrain = false;
                        depth = 0;
                        if y < WATER_HEIGHT {
                            BlockState::WATER
                        } else {
                            BlockState::AIR
                        }
                    };

                    chunk.set_block_state(offset_x, y as u32, offset_z, block);
                }
            }
        }

        // Surface heights for the decoration passes below.
        let surface = surface_heights(&chunk);
        plant_trees(&state, pos, &mut chunk, &surface);

        for offset_z in 0..16 {
            for offset_x in 0..16 {
                let x = offset_x as i32 + pos.x * 16;
                let z = offset_z as i32 + pos.z * 16;

                // Add grass on top of grass blocks.
                for y in (0..chunk.height()).rev() {
                    if chunk.block_state(offset_x, y, offset_z).is_air()
                        && chunk.block_state(offset_x, y - 1, offset_z) == BlockState::GRASS_BLOCK
                    {
                        let p = DVec3::new(f64::from(x), f64::from(y), f64::from(z));
                        let density = fbm(&state.grass, p / 5.0, 4, 2.0, 0.7);

                        if density > 0.55 {
                            if density > 0.7
                                && chunk.block_state(offset_x, y + 1, offset_z).is_air()
                            {
                                let upper =
                                    BlockState::TALL_GRASS.set(PropName::Half, PropValue::Upper);
                                let lower =
                                    BlockState::TALL_GRASS.set(PropName::Half, PropValue::Lower);

                                chunk.set_block_state(offset_x, y + 1, offset_z, upper);
                                chunk.set_block_state(offset_x, y, offset_z, lower);
                            } else {
                                chunk.set_block_state(
                                    offset_x,
                                    y,
                                    offset_z,
                                    BlockState::SHORT_GRASS,
                                );
                            }
                        }
                    }
                }
            }
        }

        let _ = state.sender.try_send((pos, chunk));
    }
}

fn has_terrain_at(state: &ChunkWorkerState, p: DVec3) -> bool {
    // Broad, gentle hills: large features, compressed amplitude and a soft
    // falloff so slopes round off instead of stacking into cliffs.
    let hilly = lerp(0.2, 0.9, noise01(&state.hilly, p / 520.0)).powi(2);

    let lower = 25.0 + 60.0 * hilly;
    let upper = lower + 55.0 * hilly;

    if p.y <= lower {
        return true;
    } else if p.y >= upper {
        return false;
    }

    let density = 1.0 - lerpstep(lower, upper, p.y);

    let n = fbm(&state.density, p / 140.0, 4, 2.0, 0.5);

    n < density
}

fn surface_heights(chunk: &UnloadedChunk) -> [[u32; 16]; 16] {
    let mut surface = [[0_u32; 16]; 16];
    for oz in 0..16 {
        for ox in 0..16 {
            for y in (0..chunk.height()).rev() {
                if !chunk.block_state(ox, y, oz).is_air() {
                    surface[oz as usize][ox as usize] = y;
                    break;
                }
            }
        }
    }
    surface
}

/// Deterministic 64-bit mix of two coordinates and the world seed.
fn hash2(x: i32, z: i32, seed: u32) -> u64 {
    let mut h = (x as u64)
        .wrapping_mul(0x8da6b3439b3f22eb)
        .wrapping_add((z as u64).wrapping_mul(0xd4f6d498f45e9d77))
        .wrapping_add(u64::from(seed));
    h ^= h >> 30;
    h = h.wrapping_mul(0xbf58476d1ce4e5b9);
    h ^= h >> 27;
    h = h.wrapping_mul(0x94d049bb133111eb);
    h ^= h >> 31;
    h
}

/// One tree candidate per 8x8 cell, gated by a low-frequency grove noise so
/// trees cluster into woods instead of peppering the landscape. Pure function
/// of world coordinates, so neighboring chunks agree without communication.
fn is_tree_candidate(state: &ChunkWorkerState, x: i32, z: i32) -> bool {
    const CELL: i32 = 8;
    let ccx = x.div_euclid(CELL);
    let ccz = z.div_euclid(CELL);
    let h = hash2(ccx, ccz, state.seed);
    let want_x = ccx * CELL + 2 + (h % (CELL as u64 - 4)) as i32;
    let want_z = ccz * CELL + 2 + ((h >> 16) % (CELL as u64 - 4)) as i32;
    if x != want_x || z != want_z {
        return false;
    }
    let grove = DVec3::new(f64::from(x) / 45.0, 0.0, f64::from(z) / 45.0);
    noise01(&state.tree, grove) >= 0.55
}

/// Plants small oaks on flat grass above the waterline. Columns whose canopy
/// would cross the chunk border are skipped because neighbor chunks generate
/// independently.
fn plant_trees(
    state: &ChunkWorkerState,
    pos: ChunkPos,
    chunk: &mut UnloadedChunk,
    surface: &[[u32; 16]; 16],
) {
    const WATER_HEIGHT: u32 = 55;
    for oz in 0..16_u32 {
        for ox in 0..16_u32 {
            let top = surface[oz as usize][ox as usize];
            if chunk.block_state(ox, top, oz) != BlockState::GRASS_BLOCK {
                continue;
            }
            if top <= WATER_HEIGHT + 1 || top + 8 >= chunk.height() {
                continue;
            }
            if !(2..=13).contains(&ox) || !(2..=13).contains(&oz) {
                continue;
            }
            // Gentle ground only: no trees on cliffs.
            let t = top as i32;
            let slope = (surface[oz as usize][(ox + 1).min(15) as usize] as i32 - t).abs()
                + (surface[oz as usize][ox.saturating_sub(1) as usize] as i32 - t).abs()
                + (surface[(oz + 1).min(15) as usize][ox as usize] as i32 - t).abs()
                + (surface[oz.saturating_sub(1) as usize][ox as usize] as i32 - t).abs();
            if slope > 4 {
                continue;
            }
            let x = ox as i32 + pos.x * 16;
            let z = oz as i32 + pos.z * 16;
            if !is_tree_candidate(state, x, z) {
                continue;
            }
            let trunk_h = 4 + (hash2(x, z, state.seed) % 2) as u32;
            let leaves = BlockState::OAK_LEAVES.set(PropName::Persistent, PropValue::True);
            for dy in trunk_h - 2..=trunk_h + 1 {
                let r = if dy < trunk_h { 2_i32 } else { 1_i32 };
                for lz in -r..=r {
                    for lx in -r..=r {
                        if lx == 0 && lz == 0 && dy <= trunk_h {
                            continue;
                        }
                        // Round the canopy: plus-shaped cap, notched wide
                        // corners.
                        if r == 1 && dy == trunk_h + 1 && lx != 0 && lz != 0 {
                            continue;
                        }
                        if r == 2
                            && lx.abs() == 2
                            && lz.abs() == 2
                            && hash2(x + lx * 31, z + lz * 57 + dy as i32, state.seed)
                                .is_multiple_of(2)
                        {
                            continue;
                        }
                        let (bx, bz, by) = (ox as i32 + lx, oz as i32 + lz, top + 1 + dy);
                        if !(0..=15).contains(&bx)
                            || !(0..=15).contains(&bz)
                            || by >= chunk.height()
                        {
                            continue;
                        }
                        if chunk.block_state(bx as u32, by, bz as u32).is_air() {
                            chunk.set_block_state(bx as u32, by, bz as u32, leaves);
                        }
                    }
                }
            }
            for i in 1..=trunk_h {
                chunk.set_block_state(ox, top + i, oz, BlockState::OAK_LOG);
            }
        }
    }
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a * (1.0 - t) + b * t
}

fn lerpstep(edge0: f64, edge1: f64, x: f64) -> f64 {
    if x <= edge0 {
        0.0
    } else if x >= edge1 {
        1.0
    } else {
        (x - edge0) / (edge1 - edge0)
    }
}

fn fbm(noise: &SuperSimplex, p: DVec3, octaves: u32, lacunarity: f64, persistence: f64) -> f64 {
    let mut freq = 1.0;
    let mut amp = 1.0;
    let mut amp_sum = 0.0;
    let mut sum = 0.0;

    for _ in 0..octaves {
        let n = noise01(noise, p * freq);
        sum += n * amp;
        amp_sum += amp;

        freq *= lacunarity;
        amp *= persistence;
    }

    // Scale the output to [0, 1]
    sum / amp_sum
}

fn noise01(noise: &SuperSimplex, p: DVec3) -> f64 {
    (noise.get(p.to_array()) + 1.0) / 2.0
}
