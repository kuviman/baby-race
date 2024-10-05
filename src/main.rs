#![allow(dead_code)]
use geng::prelude::*;

#[derive(clap::Parser)]
struct CliArgs {
    #[clap(flatten)]
    geng: geng::CliArgs,
}

#[derive(geng::asset::Load)]
struct Assets {}

struct Game {
    geng: Geng,
    assets: Rc<Assets>,
}

impl Game {
    pub fn new(geng: &Geng, assets: &Rc<Assets>) -> Self {
        Self {
            geng: geng.clone(),
            assets: assets.clone(),
        }
    }
}

impl geng::State for Game {
    fn draw(&mut self, framebuffer: &mut ugli::Framebuffer) {
        ugli::clear(framebuffer, Some(Rgba::GREEN), None, None);
    }
}

fn main() {
    let cli_args: CliArgs = cli::parse();
    Geng::run_with(
        &{
            let mut options = geng::ContextOptions::default();
            options.with_cli(&cli_args.geng);
            options
        },
        |geng: Geng| async move {
            let assets = geng
                .asset_manager()
                .load(run_dir().join("assets"))
                .await
                .expect("failed to load assets");
            geng.run_state(Game::new(&geng, &assets)).await
        },
    )
}
