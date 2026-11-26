//! Turning chunks of blocks into meshes: one face per side that can be seen,
//! with ambient occlusion at the corners.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::tasks::ComputeTaskPool;

use crate::blocks::{self, Shape, AIR, WATER};
use crate::textures::BlockAtlas;
use crate::world::{Voxels, CHUNK};

#[derive(Default)]
pub struct MeshData {
    pos: Vec<[f32; 3]>,
    nrm: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    col: Vec<[f32; 4]>,
    tan: Vec<[f32; 4]>,
    idx: Vec<u32>,
}

impl MeshData {
    pub fn is_empty(&self) -> bool {
        self.idx.is_empty()
    }

    fn quad(&mut self, corners: [[f32; 3]; 4], n: [f32; 3], rect: [f32; 4], light: [f32; 4], flip: bool) {
        let base = self.pos.len() as u32;
        let [u0, v0, u1, v1] = rect;
        let uvs = [[u0, v1], [u1, v1], [u1, v0], [u0, v0]];
        let t = Vec3::from(corners[1]) - Vec3::from(corners[0]);
        let b = Vec3::from(corners[3]) - Vec3::from(corners[0]);
        let t = t.normalize_or_zero();
        // the bitangent points up the texture (towards v0)
        let w = if Vec3::from(n).cross(t).dot(b) >= 0.0 { 1.0 } else { -1.0 };
        for k in 0..4 {
            self.pos.push(corners[k]);
            self.nrm.push(n);
            self.uv.push(uvs[k]);
            let l = light[k];
            self.col.push([l, l, l, 1.0]);
            self.tan.push([t.x, t.y, t.z, w]);
        }
        if flip {
            self.idx.extend_from_slice(&[base + 1, base + 2, base + 3, base + 1, base + 3, base]);
        } else {
            self.idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    }

    pub fn into_mesh(self) -> Mesh {
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.pos)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.nrm)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uv)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.col)
            .with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, self.tan)
            .with_inserted_indices(Indices::U32(self.idx))
    }
}

/// A chunk's three meshes: opaque blocks, cut-out blocks (leaves, glass,
/// plants) and water.
pub struct ChunkMesh {
    pub layers: [MeshData; 3],
}

struct Face {
    n: [i32; 3],
    c: [[i32; 3]; 4],
    /// Which of the block's textures: 0 top, 1 side, 2 bottom.
    s: usize,
}

const FACES: [Face; 6] = [
    Face { n: [1, 0, 0], c: [[1, 0, 1], [1, 0, 0], [1, 1, 0], [1, 1, 1]], s: 1 },
    Face { n: [-1, 0, 0], c: [[0, 0, 0], [0, 0, 1], [0, 1, 1], [0, 1, 0]], s: 1 },
    Face { n: [0, 1, 0], c: [[0, 1, 1], [1, 1, 1], [1, 1, 0], [0, 1, 0]], s: 0 },
    Face { n: [0, -1, 0], c: [[0, 0, 0], [1, 0, 0], [1, 0, 1], [0, 0, 1]], s: 2 },
    Face { n: [0, 0, 1], c: [[0, 0, 1], [1, 0, 1], [1, 1, 1], [0, 1, 1]], s: 1 },
    Face { n: [0, 0, -1], c: [[1, 0, 0], [0, 0, 0], [0, 1, 0], [1, 1, 0]], s: 1 },
];

const AO: [f32; 4] = [1.0, 0.78, 0.6, 0.45];

pub fn mesh_chunk(w: &Voxels, atlas: &BlockAtlas, c: IVec3) -> ChunkMesh {
    let mut layers = [MeshData::default(), MeshData::default(), MeshData::default()];
    let o = c * CHUNK;
    let opaque = |x: i32, y: i32, z: i32| blocks::is_opaque(w.get(x, y, z));
    for y in o.y..o.y + CHUNK {
        for z in o.z..o.z + CHUNK {
            for x in o.x..o.x + CHUNK {
                let id = w.get(x, y, z);
                if id == AIR {
                    continue;
                }
                let def = blocks::block(id);
                let (fx, fy, fz) = (x as f32, y as f32, z as f32);
                match def.shape {
                    Shape::Air => {}
                    Shape::Water => {
                        let above = w.get(x, y + 1, z);
                        if above == WATER || blocks::is_opaque(above) {
                            continue;
                        }
                        let yy = fy + 0.88;
                        let rect = BlockAtlas::uv(atlas.tile("water"));
                        layers[2].quad(
                            [[fx, yy, fz + 1.0], [fx + 1.0, yy, fz + 1.0], [fx + 1.0, yy, fz], [fx, yy, fz]],
                            [0.0, 1.0, 0.0],
                            rect,
                            [1.0; 4],
                            false,
                        );
                    }
                    Shape::Cross => {
                        let rect = BlockAtlas::uv(atlas.tile(def.faces[1]));
                        // a little offset per block so a meadow isn't a grid
                        let h = crate::noise::hash2(x, z, 77);
                        let (ox, oz) = ((h - 0.5) * 0.3, (crate::noise::hash2(z, x, 78) - 0.5) * 0.3);
                        let s = 0.45;
                        let tall = 0.75 + h * 0.3;
                        let (cx, cz) = (fx + 0.5 + ox, fz + 0.5 + oz);
                        for (dx, dz) in [(1.0f32, 1.0f32), (1.0, -1.0)] {
                            let (ax, az) = (dx * s, dz * s);
                            let a = [cx - ax, fy, cz - az];
                            let b = [cx + ax, fy, cz + az];
                            let up = [0.0, 1.0, 0.0];
                            let q = [a, b, [b[0], fy + tall, b[2]], [a[0], fy + tall, a[2]]];
                            layers[1].quad(q, up, rect, [1.0; 4], false);
                            layers[1].quad([q[1], q[0], q[3], q[2]], up, rect, [1.0; 4], false);
                        }
                    }
                    Shape::Solid | Shape::Cutout => {
                        let layer = if def.shape == Shape::Solid { 0 } else { 1 };
                        for f in &FACES {
                            let (nx, ny, nz) = (x + f.n[0], y + f.n[1], z + f.n[2]);
                            let nb = w.get(nx, ny, nz);
                            if blocks::is_opaque(nb) || (def.shape == Shape::Cutout && nb == id) {
                                continue;
                            }
                            let rect = BlockAtlas::uv(atlas.tile(def.faces[f.s]));
                            let mut light = [1.0; 4];
                            let mut occ = [0usize; 4];
                            let axes: Vec<usize> = (0..3).filter(|&i| f.n[i] == 0).collect();
                            for k in 0..4 {
                                let cc = f.c[k];
                                let step = |i: usize| if cc[i] == 1 { 1 } else { -1 };
                                let mut s1 = [nx, ny, nz];
                                s1[axes[0]] += step(axes[0]);
                                let mut s2 = [nx, ny, nz];
                                s2[axes[1]] += step(axes[1]);
                                let mut s3 = [nx, ny, nz];
                                s3[axes[0]] += step(axes[0]);
                                s3[axes[1]] += step(axes[1]);
                                let a = opaque(s1[0], s1[1], s1[2]) as usize;
                                let b = opaque(s2[0], s2[1], s2[2]) as usize;
                                let d = opaque(s3[0], s3[1], s3[2]) as usize;
                                occ[k] = if a == 1 && b == 1 { 3 } else { a + b + d };
                                if !def.light {
                                    light[k] = AO[occ[k]];
                                }
                            }
                            let corners = f.c.map(|cc| [fx + cc[0] as f32, fy + cc[1] as f32, fz + cc[2] as f32]);
                            let n = f.n.map(|v| v as f32);
                            let flip = occ[0] + occ[2] > occ[1] + occ[3];
                            layers[layer].quad(corners, n, rect, light, flip);
                        }
                    }
                }
            }
        }
    }
    ChunkMesh { layers }
}

#[derive(Resource)]
pub struct BlockMaterials {
    pub layers: [Handle<StandardMaterial>; 3],
}

#[derive(Resource, Default)]
pub struct ChunkEntities {
    pub map: HashMap<IVec3, [Option<Entity>; 3]>,
    /// How many chunks have been meshed since the world loaded.
    pub meshed: usize,
}

#[derive(Component)]
pub struct ChunkPart;

/// The point chunks are meshed outward from (the player's eye).
#[derive(Resource, Default)]
pub struct MeshFocus(pub Vec3);

/// Meshes the dirty chunks nearest the player, several at once on the
/// task pool, a few per frame so the game never stalls.
pub fn remesh(
    mut commands: Commands,
    mut voxels: ResMut<Voxels>,
    atlas: Res<BlockAtlas>,
    mats: Res<BlockMaterials>,
    focus: Res<MeshFocus>,
    mut chunks: ResMut<ChunkEntities>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    if voxels.dirty.is_empty() {
        return;
    }
    let f = (focus.0 / CHUNK as f32).floor().as_ivec3();
    let mut list: Vec<IVec3> = voxels.dirty.iter().copied().collect();
    list.sort_by_key(|c| {
        let d = *c - f;
        d.x * d.x + d.y * d.y * 2 + d.z * d.z
    });
    // more per frame while the world first loads
    let budget = if chunks.meshed < 400 { 48 } else { 12 };
    list.truncate(budget);
    for c in &list {
        voxels.dirty.remove(c);
    }
    let w = &*voxels;
    let a = &*atlas;
    let done: Vec<(IVec3, ChunkMesh)> = ComputeTaskPool::get().scope(|s| {
        for &c in &list {
            s.spawn(async move { (c, mesh_chunk(w, a, c)) });
        }
    });
    for (c, m) in done {
        chunks.meshed += 1;
        let slot = chunks.map.entry(c).or_insert([None; 3]);
        for (i, data) in m.layers.into_iter().enumerate() {
            if let Some(e) = slot[i].take() {
                commands.entity(e).despawn();
            }
            if data.is_empty() {
                continue;
            }
            let mut e = commands.spawn((
                Mesh3d(meshes.add(data.into_mesh())),
                MeshMaterial3d(mats.layers[i].clone()),
                Transform::IDENTITY,
                ChunkPart,
            ));
            if i == 2 {
                e.insert(bevy::light::NotShadowCaster);
            }
            slot[i] = Some(e.id());
        }
    }
}
