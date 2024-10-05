#![allow(dead_code)]
use geng::prelude::*;

#[derive(clap::Parser)]
struct CliArgs {
    #[clap(flatten)]
    geng: geng::CliArgs,
}

#[derive(Deserialize)]
struct BabyConfig {
    radius: f32,
    head_offset: vec2<f32>,
    arm_offset: vec2<f32>,
    leg_offset: vec2<f32>,
    limb_rotation_limit: f32,
}

#[derive(geng::asset::Load, Deserialize)]
#[load(serde = "toml")]
struct Config {
    background_color: Rgba<f32>,
    fov: f32,
    baby: BabyConfig,
}

#[derive(Deref)]
struct Texture(ugli::Texture);

impl std::borrow::Borrow<ugli::Texture> for &Texture {
    fn borrow(&self) -> &ugli::Texture {
        &self.0
    }
}

impl geng::asset::Load for Texture {
    type Options = ();
    fn load(
        manager: &geng::asset::Manager,
        path: &std::path::Path,
        _options: &Self::Options,
    ) -> geng::asset::Future<Self> {
        let texture = manager.load(path);
        async move {
            let mut texture: ugli::Texture = texture.await?;
            texture.set_filter(ugli::Filter::Nearest);
            Ok::<_, anyhow::Error>(Self(texture))
        }
        .boxed_local()
    }

    const DEFAULT_EXT: Option<&'static str> = Some("png");
}

#[derive(geng::asset::Load)]
struct BabyAssets {
    body: Texture,
    head: Texture,
    arm: Texture,
    leg: Texture,
}

#[derive(geng::asset::Load)]
struct Assets {
    config: Config,
    baby: BabyAssets,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
enum Limb {
    LeftArm,
    RightArm,
    LeftLeg,
    RightLeg,
}

struct LimbState {
    rotation: Angle<f32>,
}

struct Baby {
    pos: vec2<f32>,
    rotation: Angle<f32>,
    radius: f32,
    limbs: HashMap<Limb, LimbState>,
}

impl Baby {
    fn new(assets: &Assets, pos: vec2<f32>) -> Self {
        Self {
            pos,
            rotation: Angle::ZERO,
            radius: assets.config.baby.radius,
            limbs: {
                let mut map = HashMap::new();
                for limb in [Limb::LeftArm, Limb::RightArm, Limb::LeftLeg, Limb::RightLeg] {
                    map.insert(
                        limb,
                        LimbState {
                            rotation: Angle::ZERO,
                        },
                    );
                }
                map
            },
        }
    }
}

struct Game {
    geng: Geng,
    assets: Rc<Assets>,
    baby: Baby,
    camera: Camera2d,
    time: f32,
}

impl Game {
    pub fn new(geng: &Geng, assets: &Rc<Assets>) -> Self {
        Self {
            geng: geng.clone(),
            assets: assets.clone(),
            baby: Baby::new(assets, vec2::ZERO),
            camera: Camera2d {
                center: vec2::ZERO,
                rotation: Angle::ZERO,
                fov: Camera2dFov::MinSide(assets.config.fov),
            },
            time: 0.0,
        }
    }

    fn draw_baby(&self, framebuffer: &mut ugli::Framebuffer, baby: &Baby) {
        let transform = mat3::translate(baby.pos)
            * mat3::rotate(baby.rotation)
            * mat3::scale_uniform(baby.radius);
        self.geng.draw2d().draw2d(
            framebuffer,
            &self.camera,
            &draw2d::TexturedQuad::unit(&self.assets.baby.arm).transform(
                transform
                    * mat3::translate(self.assets.config.baby.arm_offset)
                    * mat3::rotate(baby.limbs[&Limb::LeftArm].rotation),
            ),
        );
        self.geng.draw2d().draw2d(
            framebuffer,
            &self.camera,
            &draw2d::TexturedQuad::unit(&self.assets.baby.arm).transform(
                transform
                    * mat3::scale(vec2(-1.0, 1.0))
                    * mat3::translate(self.assets.config.baby.arm_offset)
                    * mat3::rotate(-baby.limbs[&Limb::RightArm].rotation),
            ),
        );
        self.geng.draw2d().draw2d(
            framebuffer,
            &self.camera,
            &draw2d::TexturedQuad::unit(&self.assets.baby.leg).transform(
                transform
                    * mat3::translate(self.assets.config.baby.leg_offset)
                    * mat3::rotate(baby.limbs[&Limb::LeftLeg].rotation),
            ),
        );
        self.geng.draw2d().draw2d(
            framebuffer,
            &self.camera,
            &draw2d::TexturedQuad::unit(&self.assets.baby.leg).transform(
                transform
                    * mat3::scale(vec2(-1.0, 1.0))
                    * mat3::translate(self.assets.config.baby.leg_offset)
                    * mat3::rotate(-baby.limbs[&Limb::RightLeg].rotation),
            ),
        );
        self.geng.draw2d().draw2d(
            framebuffer,
            &self.camera,
            &draw2d::TexturedQuad::unit(&self.assets.baby.body).transform(transform),
        );
        self.geng.draw2d().draw2d(
            framebuffer,
            &self.camera,
            &draw2d::TexturedQuad::unit(&self.assets.baby.head)
                .transform(transform * mat3::translate(self.assets.config.baby.head_offset)),
        );
    }
}

impl geng::State for Game {
    fn draw(&mut self, framebuffer: &mut ugli::Framebuffer) {
        ugli::clear(
            framebuffer,
            Some(self.assets.config.background_color),
            None,
            None,
        );
        self.draw_baby(framebuffer, &self.baby);
    }
    fn update(&mut self, delta_time: f64) {
        let delta_time = delta_time as f32;
        self.time += delta_time;
        for limb in self.baby.limbs.values_mut() {
            limb.rotation =
                Angle::from_degrees(self.time.sin() * self.assets.config.baby.limb_rotation_limit);
        }
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
