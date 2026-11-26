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
}

pub struct Block {
    pub name: &'static str,
    /// Texture names for the top, the sides and the bottom.
    pub faces: [&'static str; 3],
    pub shape: Shape,
    /// Gives light (lanterns).
    pub light: bool,
    /// Seconds to break by hand; 0 is instant, negative can't be broken.
    pub hardness: f32,
}

const fn b(name: &'static str, faces: [&'static str; 3], shape: Shape, hardness: f32) -> Block {
    Block { name, faces, shape, light: false, hardness }
}

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
    Block { name: "Lantern", faces: ["lantern", "lantern", "lantern"], shape: Shape::Solid, light: true, hardness: 0.3 },
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
];

pub fn block(id: BlockId) -> &'static Block {
    BLOCKS.get(id as usize).unwrap_or(&BLOCKS[0])
}

pub fn shape(id: BlockId) -> Shape {
    block(id).shape
}

/// Bodies can't pass through it.
pub fn is_solid(id: BlockId) -> bool {
    matches!(shape(id), Shape::Solid | Shape::Cutout)
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
