//! The base game's blocks: what each looks like and how it behaves.
//!
//! Block ids are bytes, stored in chunks and sent over the network, so never
//! renumber one; add new blocks at the end.

pub type BlockId = u8;

pub const AIR: BlockId = 0;
pub const GRASS: BlockId = 1;
pub const DIRT: BlockId = 2;
pub const STONE: BlockId = 3;
pub const SAND: BlockId = 4;
pub const GRAVEL: BlockId = 5;
pub const LOG: BlockId = 6;
pub const LEAVES: BlockId = 7;
pub const PLANKS: BlockId = 8;
pub const MOSSY: BlockId = 9;
pub const SNOW: BlockId = 10;
pub const WOOL_RED: BlockId = 11;
pub const WOOL_CREAM: BlockId = 12;
pub const HAY: BlockId = 13;
pub const PINE: BlockId = 14;
pub const WOOL_BLUE: BlockId = 15;
pub const WOOL_YELLOW: BlockId = 16;
pub const WATER: BlockId = 17;
pub const LANTERN: BlockId = 18;
pub const BIRCH: BlockId = 19;
pub const BIRCH_LEAVES: BlockId = 20;
pub const COBBLE: BlockId = 21;
pub const TALLGRASS: BlockId = 22;
pub const FLOWER_RED: BlockId = 23;
pub const FLOWER_BLUE: BlockId = 24;
pub const FLOWER_WHITE: BlockId = 25;
pub const FLOWER_VIOLET: BlockId = 26;
pub const GLASS: BlockId = 27;
pub const BEDROCK: BlockId = 28;
pub const FERN: BlockId = 29;
pub const BUSH: BlockId = 30;
pub const BARREL: BlockId = 31;
pub const CRATE: BlockId = 32;
pub const BOOKSHELF: BlockId = 33;
pub const CANDLE: BlockId = 34;
pub const STAKE: BlockId = 35;
pub const WHEAT: BlockId = 36;
pub const CARROTS: BlockId = 37;
pub const FARMLAND: BlockId = 38;
pub const POT: BlockId = 39;
pub const TABLE: BlockId = 40;
pub const STOOL: BlockId = 41;
pub const SANDSTONE: BlockId = 42;
pub const SANDSTONE_BRICKS: BlockId = 43;
pub const COLUMN: BlockId = 44;
pub const BRAZIER: BlockId = 45;
pub const FENCE: BlockId = 46;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shape {
    Air,
    /// A full cube that hides the faces behind it.
    Solid,
    /// A full cube with holes (leaves, glass): drawn, but neighbours still show.
    Cutout,
    /// Two crossed quads (grass, flowers); not solid.
    Cross,
    Water,
    /// Boxes smaller than the block (barrels, tables, candles): see `Cuboid`.
    Model,
}

pub struct Block {
    pub name: &'static str,
    /// Texture names for the top, the sides and the bottom.
    pub faces: [&'static str; 3],
    pub shape: Shape,
    /// Gives light (lanterns, candles, braziers).
    pub light: bool,
    /// Seconds to break by hand; 0 is instant, negative can't be broken.
    pub hardness: f32,
    /// A model's boxes.
    pub model: &'static [Cuboid],
    /// Whether bodies bump into a model (a table, but not a candle).
    pub solid: bool,
}

/// Which way a side faces: east, west, up, down, south, north.
pub const SIDES: [[i32; 3]; 6] = [[1, 0, 0], [-1, 0, 0], [0, 1, 0], [0, -1, 0], [0, 0, 1], [0, 0, -1]];

/// One box of a block model, in sixteenths of a block, as Minecraft's are.
/// Each face shows the part of its texture it covers, so a model's side
/// texture is drawn as a picture of the model from the side.
#[derive(Clone, Copy)]
pub struct Cuboid {
    pub from: [u8; 3],
    pub to: [u8; 3],
    /// Textures for the top, the sides and the bottom; "" takes the block's.
    pub tex: [&'static str; 3],
    /// Glows (a flame, coals); the glow map says which pixels.
    pub glow: bool,
    pub when: When,
}

/// When a box of a model shows, by what's next to the block (indices into `SIDES`).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum When {
    Always,
    /// Only when the block that way isn't the same: a column's base and capital.
    Alone(usize),
    /// Only when the block that way is the same: a column's shaft going on.
    Same(usize),
    /// Only when the block that way is the same or a full block: a fence's rails.
    Joins(usize),
}

impl When {
    /// Whether a box shows in block `id`, given what's next to it.
    pub fn holds(self, id: BlockId, next: impl Fn(usize) -> BlockId) -> bool {
        match self {
            When::Always => true,
            When::Alone(d) => next(d) != id,
            When::Same(d) => next(d) == id,
            When::Joins(d) => {
                let n = next(d);
                n == id || is_opaque(n)
            }
        }
    }
}

impl Cuboid {
    pub const fn new(from: [u8; 3], to: [u8; 3]) -> Self {
        Cuboid { from, to, tex: ["", "", ""], glow: false, when: When::Always }
    }

    pub const fn tex(mut self, tex: [&'static str; 3]) -> Self {
        self.tex = tex;
        self
    }

    pub const fn glow(mut self) -> Self {
        self.glow = true;
        self
    }

    pub const fn when(mut self, when: When) -> Self {
        self.when = when;
        self
    }
}

const fn b(name: &'static str, faces: [&'static str; 3], shape: Shape, hardness: f32) -> Block {
    Block { name, faces, shape, light: false, hardness, model: &[], solid: false }
}

/// A block made of boxes.
const fn m(name: &'static str, faces: [&'static str; 3], model: &'static [Cuboid], solid: bool, hardness: f32) -> Block {
    Block { name, faces, shape: Shape::Model, light: false, hardness, model, solid }
}

const fn lit(mut b: Block) -> Block {
    b.light = true;
    b
}

const fn bx(from: [u8; 3], to: [u8; 3]) -> Cuboid {
    Cuboid::new(from, to)
}

// the models' boxes
const BARREL_BOXES: &[Cuboid] = &[bx([2, 0, 1], [14, 16, 15]), bx([1, 0, 2], [15, 16, 14])];
const CANDLE_BOXES: &[Cuboid] = &[
    bx([6, 0, 6], [10, 1, 10]).tex(["candle_dish", "candle_dish", "candle_dish"]),
    bx([7, 1, 7], [9, 7, 9]),
    bx([7, 7, 7], [9, 11, 9]).tex(["flame", "flame", "flame"]).glow(),
];
const STAKE_BOXES: &[Cuboid] = &[
    bx([4, 0, 4], [12, 12, 12]),
    bx([4, 12, 4], [12, 16, 12]).when(When::Same(2)),
    bx([5, 12, 5], [11, 14, 11]).when(When::Alone(2)),
    bx([6, 14, 6], [10, 15, 10]).when(When::Alone(2)),
    bx([7, 15, 7], [9, 16, 9]).when(When::Alone(2)),
];
const POT_BOXES: &[Cuboid] = &[bx([4, 0, 4], [12, 8, 12]), bx([3, 8, 3], [13, 10, 13])];
const TABLE_BOXES: &[Cuboid] = &[
    bx([0, 13, 0], [16, 16, 16]),
    bx([1, 0, 1], [3, 13, 3]),
    bx([13, 0, 1], [15, 13, 3]),
    bx([1, 0, 13], [3, 13, 15]),
    bx([13, 0, 13], [15, 13, 15]),
];
const STOOL_BOXES: &[Cuboid] = &[
    bx([3, 7, 3], [13, 9, 13]),
    bx([4, 0, 4], [6, 7, 6]),
    bx([10, 0, 4], [12, 7, 6]),
    bx([4, 0, 10], [6, 7, 12]),
    bx([10, 0, 10], [12, 7, 12]),
];
const COLUMN_BOXES: &[Cuboid] = &[
    bx([3, 3, 3], [13, 13, 13]),
    bx([3, 0, 3], [13, 3, 13]).tex(["", "column_shaft", ""]).when(When::Same(3)),
    bx([3, 13, 3], [13, 16, 13]).tex(["", "column_shaft", ""]).when(When::Same(2)),
    bx([1, 0, 1], [15, 3, 15]).when(When::Alone(3)),
    bx([1, 13, 1], [15, 16, 15]).when(When::Alone(2)),
];
const BRAZIER_BOXES: &[Cuboid] = &[
    bx([2, 6, 2], [14, 11, 14]),
    bx([3, 11, 3], [13, 12, 13]).tex(["coals", "coals", "coals"]).glow(),
    bx([3, 0, 3], [5, 6, 5]),
    bx([11, 0, 3], [13, 6, 5]),
    bx([3, 0, 11], [5, 6, 13]),
    bx([11, 0, 11], [13, 6, 13]),
];
const FENCE_BOXES: &[Cuboid] = &[
    bx([6, 0, 6], [10, 16, 10]).tex(["log_top", "", "log_top"]),
    bx([10, 6, 7], [16, 9, 9]).when(When::Joins(0)),
    bx([10, 11, 7], [16, 14, 9]).when(When::Joins(0)),
    bx([0, 6, 7], [6, 9, 9]).when(When::Joins(1)),
    bx([0, 11, 7], [6, 14, 9]).when(When::Joins(1)),
    bx([7, 6, 10], [9, 9, 16]).when(When::Joins(4)),
    bx([7, 11, 10], [9, 14, 16]).when(When::Joins(4)),
    bx([7, 6, 0], [9, 9, 6]).when(When::Joins(5)),
    bx([7, 11, 0], [9, 14, 6]).when(When::Joins(5)),
];

pub static BLOCKS: &[Block] = &[
    b("Air", ["", "", ""], Shape::Air, 0.0),
    b("Grass", ["grass_top", "grass_side", "dirt"], Shape::Solid, 0.5),
    b("Dirt", ["dirt", "dirt", "dirt"], Shape::Solid, 0.45),
    b("Stone", ["stone", "stone", "stone"], Shape::Solid, 1.2),
    b("Sand", ["sand", "sand", "sand"], Shape::Solid, 0.4),
    b("Gravel", ["gravel", "gravel", "gravel"], Shape::Solid, 0.5),
    b("Oak log", ["log_top", "log_side", "log_top"], Shape::Solid, 1.0),
    b("Oak leaves", ["leaves", "leaves", "leaves"], Shape::Cutout, 0.15),
    b("Planks", ["planks", "planks", "planks"], Shape::Solid, 0.8),
    b("Mossy brick", ["mossy_brick", "mossy_brick", "mossy_brick"], Shape::Solid, 1.4),
    b("Snow", ["snow", "snow_side", "stone"], Shape::Solid, 0.4),
    b("Red wool", ["wool_red", "wool_red", "wool_red"], Shape::Solid, 0.3),
    b("Cream wool", ["wool_cream", "wool_cream", "wool_cream"], Shape::Solid, 0.3),
    b("Hay", ["hay_top", "hay_side", "hay_top"], Shape::Solid, 0.3),
    b("Pine needles", ["pine", "pine", "pine"], Shape::Cutout, 0.15),
    b("Blue wool", ["wool_blue", "wool_blue", "wool_blue"], Shape::Solid, 0.3),
    b("Yellow wool", ["wool_yellow", "wool_yellow", "wool_yellow"], Shape::Solid, 0.3),
    b("Water", ["water", "water", "water"], Shape::Water, -1.0),
    lit(b("Lantern", ["lantern", "lantern", "lantern"], Shape::Solid, 0.3)),
    b("Birch log", ["log_top", "birch_side", "log_top"], Shape::Solid, 1.0),
    b("Birch leaves", ["birch_leaves", "birch_leaves", "birch_leaves"], Shape::Cutout, 0.15),
    b("Cobblestone", ["cobble", "cobble", "cobble"], Shape::Solid, 1.3),
    b("Tall grass", ["tallgrass", "tallgrass", "tallgrass"], Shape::Cross, 0.0),
    b("Red flower", ["flower_red", "flower_red", "flower_red"], Shape::Cross, 0.0),
    b("Blue flower", ["flower_blue", "flower_blue", "flower_blue"], Shape::Cross, 0.0),
    b("White flower", ["flower_white", "flower_white", "flower_white"], Shape::Cross, 0.0),
    b("Violet flower", ["flower_violet", "flower_violet", "flower_violet"], Shape::Cross, 0.0),
    b("Glass", ["glass", "glass", "glass"], Shape::Cutout, 0.3),
    b("Bedrock", ["bedrock", "bedrock", "bedrock"], Shape::Solid, -1.0),
    b("Fern", ["fern", "fern", "fern"], Shape::Cross, 0.0),
    b("Bush", ["bush", "bush", "bush"], Shape::Cross, 0.1),
    m("Barrel", ["barrel_top", "barrel_side", "barrel_top"], BARREL_BOXES, true, 0.8),
    b("Crate", ["crate", "crate", "crate"], Shape::Solid, 0.6),
    b("Bookshelf", ["planks", "bookshelf", "planks"], Shape::Solid, 0.8),
    lit(m("Candle", ["candle", "candle", "candle"], CANDLE_BOXES, false, 0.0)),
    m("Stake", ["stake_top", "stake", "stake_top"], STAKE_BOXES, true, 0.9),
    b("Wheat", ["wheat", "wheat", "wheat"], Shape::Cross, 0.0),
    b("Carrots", ["carrots", "carrots", "carrots"], Shape::Cross, 0.0),
    b("Farmland", ["farmland", "dirt", "dirt"], Shape::Solid, 0.45),
    m("Clay pot", ["pot_top", "pot", "pot_top"], POT_BOXES, false, 0.3),
    m("Table", ["planks", "table", "planks"], TABLE_BOXES, true, 0.6),
    m("Stool", ["planks", "stool", "planks"], STOOL_BOXES, true, 0.4),
    b("Sandstone", ["sandstone_top", "sandstone", "sandstone_top"], Shape::Solid, 1.0),
    b("Sandstone bricks", ["sandstone_top", "sandstone_bricks", "sandstone_top"], Shape::Solid, 1.1),
    m("Column", ["column_top", "column", "column_top"], COLUMN_BOXES, true, 1.3),
    lit(m("Brazier", ["brazier_top", "brazier", "brazier_top"], BRAZIER_BOXES, true, 0.8)),
    m("Fence", ["planks", "fence", "planks"], FENCE_BOXES, true, 0.6),
];

pub fn block(id: BlockId) -> &'static Block {
    BLOCKS.get(id as usize).unwrap_or(&BLOCKS[0])
}

pub fn shape(id: BlockId) -> Shape {
    block(id).shape
}

/// Bodies can't pass through it.
pub fn is_solid(id: BlockId) -> bool {
    let b = block(id);
    matches!(b.shape, Shape::Solid | Shape::Cutout) || (b.shape == Shape::Model && b.solid)
}

/// Hides the faces of the blocks next to it.
pub fn is_opaque(id: BlockId) -> bool {
    shape(id) == Shape::Solid
}

/// The blocks a new player starts with in the hotbar.
pub const STARTER_HOTBAR: [BlockId; 9] =
    [GRASS, PLANKS, COBBLE, WOOL_RED, LANTERN, HAY, LOG, GLASS, MOSSY];

/// Looks a block up by its name (as creations write it), ignoring case and
/// spaces: "mossy brick", "Mossy_Brick" and "mossybrick" all match.
pub fn by_name(name: &str) -> Option<BlockId> {
    let key: String = name.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase();
    BLOCKS.iter().position(|b| {
        b.name.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase() == key
    }).map(|i| i as BlockId)
}
