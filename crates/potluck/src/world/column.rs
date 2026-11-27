//! A column of the world: CHUNK x HEIGHT x CHUNK blocks, kept as eight
//! sections of a chunk each. A section all of one block (the air above the
//! hills, the stone deep down) is kept as that block alone, and the rest are
//! shared until they change, so copying a column (a test's fresh world, the
//! island every world starts from) costs almost nothing.

use std::sync::Arc;

use crate::blocks::{self, BlockId, AIR};

use super::{CHUNK, MIN_Y, SECTIONS};

/// Blocks in a section.
pub const VOLUME: usize = (CHUNK * CHUNK * CHUNK) as usize;
/// Blocks in a column.
pub const BLOCKS: usize = VOLUME * SECTIONS;

#[derive(Clone)]
pub enum Section {
    /// All one block.
    All(BlockId),
    /// Blocks by (y, z, x), shared until one changes.
    Mixed(Arc<[BlockId; VOLUME]>),
}

#[derive(Clone)]
pub struct Column {
    pub sections: [Section; SECTIONS],
    /// A bit for each section known to be solid blocks right through, which
    /// with solid all round it has nothing to draw (the deep rock).
    solid: u8,
}

#[inline]
fn local(lx: i32, ly: i32, lz: i32) -> usize {
    ((ly * CHUNK + lz) * CHUNK + lx) as usize
}

impl Default for Column {
    fn default() -> Self {
        Column { sections: std::array::from_fn(|_| Section::All(AIR)), solid: 0 }
    }
}

impl Column {
    /// The block at (lx, y, lz): x and z inside the column, y in the world.
    #[inline]
    pub fn get(&self, lx: i32, y: i32, lz: i32) -> BlockId {
        let sy = y - MIN_Y;
        match &self.sections[(sy >> 5) as usize] {
            Section::All(b) => *b,
            Section::Mixed(d) => d[local(lx, sy & (CHUNK - 1), lz)],
        }
    }

    #[inline]
    pub fn put(&mut self, lx: i32, y: i32, lz: i32, id: BlockId) {
        let sy = y - MIN_Y;
        if !blocks::is_opaque(id) {
            self.solid &= !(1 << (sy >> 5));
        }
        let s = &mut self.sections[(sy >> 5) as usize];
        let i = local(lx, sy & (CHUNK - 1), lz);
        match s {
            Section::All(b) if *b == id => {}
            Section::All(b) => {
                let mut d = Arc::new([*b; VOLUME]);
                Arc::make_mut(&mut d)[i] = id;
                *s = Section::Mixed(d);
            }
            Section::Mixed(d) => {
                if d[i] != id {
                    Arc::make_mut(d)[i] = id;
                }
            }
        }
    }

    /// The section at index `i` (0 at the bottom of the world) when it's all
    /// one block.
    pub fn uniform(&self, i: usize) -> Option<BlockId> {
        match &self.sections[i] {
            Section::All(b) => Some(*b),
            Section::Mixed(_) => None,
        }
    }

    /// Whether section `i` is solid blocks right through (as far as is
    /// known: sections players fill in aren't counted).
    pub fn solid(&self, i: usize) -> bool {
        self.solid & (1 << i) != 0
    }

    /// Sections that came out all one block go back to being kept as one,
    /// and which are solid right through is worked out again.
    pub fn compact(&mut self) {
        self.solid = 0;
        for (i, s) in self.sections.iter_mut().enumerate() {
            if let Section::Mixed(d) = s {
                let first = d[0];
                if d.iter().all(|&b| b == first) {
                    *s = Section::All(first);
                }
            }
            let through = match s {
                Section::All(b) => blocks::is_opaque(*b),
                Section::Mixed(d) => d.iter().all(|&b| blocks::is_opaque(b)),
            };
            if through {
                self.solid |= 1 << i;
            }
        }
    }

    /// Calls `f` with every block that isn't in an all-one section, by its
    /// place in the column (lx, y, lz), and also for all-one sections of a
    /// block `wanted` says yes to.
    pub fn each(&self, mut wanted: impl FnMut(BlockId) -> bool, mut f: impl FnMut(i32, i32, i32, BlockId)) {
        for (si, s) in self.sections.iter().enumerate() {
            let y0 = MIN_Y + si as i32 * CHUNK;
            match s {
                Section::All(b) => {
                    if wanted(*b) {
                        for ly in 0..CHUNK {
                            for lz in 0..CHUNK {
                                for lx in 0..CHUNK {
                                    f(lx, y0 + ly, lz, *b);
                                }
                            }
                        }
                    }
                }
                Section::Mixed(d) => {
                    for (i, &b) in d.iter().enumerate() {
                        let i = i as i32;
                        if wanted(b) {
                            f(i % CHUNK, y0 + i / (CHUNK * CHUNK), (i / CHUNK) % CHUNK, b);
                        }
                    }
                }
            }
        }
    }

    /// Gives blocks new ids (`made::restore`).
    pub fn renumber(&mut self, to: &[BlockId; 256]) {
        for s in self.sections.iter_mut() {
            match s {
                Section::All(b) => *b = to[*b as usize],
                Section::Mixed(d) => {
                    if d.iter().any(|&b| to[b as usize] != b) {
                        for b in Arc::make_mut(d).iter_mut() {
                            *b = to[*b as usize];
                        }
                    }
                }
            }
        }
    }

    /// Bytes this column holds of its own (shared sections count in full).
    pub fn bytes(&self) -> usize {
        self.sections.iter().map(|s| if matches!(s, Section::Mixed(_)) { VOLUME } else { 1 }).sum()
    }

    /// The column run-length encoded, by (y, z, x) from the bottom: each run
    /// a block and how many (u16).
    pub fn encode(&self, out: &mut Vec<u8>) {
        let mut run: Option<(BlockId, u32)> = None;
        let flush = |out: &mut Vec<u8>, b: BlockId, mut n: u32| {
            while n > 0 {
                let k = n.min(65535);
                out.push(b);
                out.extend_from_slice(&(k as u16).to_le_bytes());
                n -= k;
            }
        };
        let mut add = |out: &mut Vec<u8>, b: BlockId, n: u32| match &mut run {
            Some((rb, rn)) if *rb == b => *rn += n,
            _ => {
                if let Some((rb, rn)) = run.take() {
                    flush(out, rb, rn);
                }
                run = Some((b, n));
            }
        };
        for s in &self.sections {
            match s {
                Section::All(b) => add(out, *b, VOLUME as u32),
                Section::Mixed(d) => {
                    for &b in d.iter() {
                        add(out, b, 1);
                    }
                }
            }
        }
        if let Some((rb, rn)) = run {
            flush(out, rb, rn);
        }
    }

    /// Reads a column `encode` wrote, or None if the runs don't add up.
    pub fn decode(bytes: &[u8]) -> Option<Column> {
        let mut flat = vec![AIR; BLOCKS];
        let mut i = 0usize;
        if bytes.len() % 3 != 0 {
            return None;
        }
        for run in bytes.chunks_exact(3) {
            let n = u16::from_le_bytes([run[1], run[2]]) as usize;
            if i + n > BLOCKS {
                return None;
            }
            flat[i..i + n].fill(run[0]);
            i += n;
        }
        (i == BLOCKS).then(|| Column::from_flat(&flat))
    }

    /// A column from its blocks by (y, z, x) from the bottom.
    pub fn from_flat(flat: &[BlockId]) -> Column {
        let mut c = Column { sections: std::array::from_fn(|si| Section::Mixed(Arc::new(flat[si * VOLUME..(si + 1) * VOLUME].try_into().unwrap()))), solid: 0 };
        c.compact();
        c
    }
}
