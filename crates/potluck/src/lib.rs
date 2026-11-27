//! Potluck: a block sandbox where everything is made by someone you know.
//! The game (`potluck`) and the dedicated server (`potluck-server`) share
//! everything here; only the game draws.

pub mod blocks;
pub mod creations;
pub mod creatures;
pub mod demo;
pub mod game;
pub mod green;
pub mod halves;
pub mod hosting;
pub mod input;
pub mod mesh;
pub mod net;
pub mod noise;
pub mod player;
pub mod render;
pub mod script;
pub mod settings;
pub mod share;
pub mod sound;
pub mod textures;
pub mod transport;
pub mod ui;
pub mod world;

use bevy::prelude::*;

/// Knob changes from Tweak this frame: (creation, knob, value).
#[derive(Resource, Default)]
pub struct KnobEdits(pub Vec<(String, String, script::KnobValue)>);

/// The world's seed when nobody says otherwise.
pub const SEED: u32 = 26;

/// Everything a world needs whether or not anyone draws it: the blocks,
/// creations and their creatures, and friends coming and going.
pub fn sim(app: &mut App, seed: u32) {
    let lib = creations::Library::load();
    app.insert_resource(world::generate(seed))
        .insert_resource(lib)
        .insert_resource(game::HerdRng(noise::Rng::new(seed)))
        .init_resource::<creatures::Herd>()
        .init_resource::<script::Scripts>()
        .init_resource::<net::Net>()
        .init_resource::<game::Actions>()
        .init_resource::<ui::Toasts>()
        .init_resource::<ui::Focus>()
        .init_resource::<ui::Acts>()
        .init_resource::<ui::WorldInfo>()
        .init_resource::<KnobEdits>()
        .init_resource::<render::Clock>()
        .init_resource::<share::ShareJobs>()
        .init_resource::<green::Green>()
        .init_resource::<hosting::TickTimes>()
        .init_resource::<sound::Heard>()
        .init_resource::<halves::Halves>()
        .add_systems(First, hosting::tick_begins)
        .add_systems(Last, hosting::tick_ends)
        .add_systems(Startup, start)
        .add_systems(Update, (halves::run, player::report_pockets).before(Sim))
        .add_systems(
            Update,
            (game::host_step, net::host_net, game::reloads, game::share_results).chain().in_set(Sim),
        );
    // sounds, where there's a speaker: the game, not the server
    if app.is_plugin_added::<bevy::audio::AudioPlugin>() {
        sound::add(app);
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct Sim;

fn start(
    mut lib: ResMut<creations::Library>,
    mut scripts: ResMut<script::Scripts>,
    mut herd: ResMut<creatures::Herd>,
    mut net: ResMut<net::Net>,
    mut toasts: ResMut<ui::Toasts>,
) {
    game::start_world(&mut lib, &mut scripts, &mut herd, &mut net, &mut toasts);
}

#[cfg(test)]
mod tests;
