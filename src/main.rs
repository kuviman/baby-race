#![allow(dead_code)]
use geng::prelude::*;

#[derive(clap::Parser)]
struct CliArgs {
    #[clap(flatten)]
    geng: geng::CliArgs,
}

#[derive(Deserialize)]
struct LimbConfig {
    /// degrees
    angle: f32,
    /// Where we are attached to the body
    body_pos: vec2<f32>,
    /// relative to body_pos
    touch_ground: vec2<f32>,
    /// wether to flip the texture
    flip: bool,
}

#[derive(Deserialize)]
struct BabyConfig {
    radius: f32,
    head_offset: vec2<f32>,
    limb_rotation_limit: f32,
    limb_length: f32,
    max_head_rotation: f32,
    head_rotation_k: f32,
    limbs: HashMap<Limb, LimbConfig>,
}

#[derive(geng::asset::Load, Deserialize)]
#[load(serde = "toml")]
struct Config {
    background_color: Rgba<f32>,
    fov: f32,
    sensitivity: f32,
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

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Deserialize)]
enum Limb {
    LeftArm,
    RightArm,
    LeftLeg,
    RightLeg,
}

impl Limb {
    fn is_leg(&self) -> bool {
        match self {
            Limb::LeftArm | Limb::RightArm => false,
            Limb::LeftLeg | Limb::RightLeg => true,
        }
    }
    fn all() -> impl Iterator<Item = Self> {
        [Self::LeftArm, Self::RightArm, Self::LeftLeg, Self::RightLeg].into_iter()
    }
}

struct LimbState {
    rotation: Angle<f32>,
    angle: Angle<f32>,
}

struct Baby {
    pos: vec2<f32>,
    rotation: Angle<f32>,
    head_rotation: Angle<f32>,
    radius: f32,
    limbs: HashMap<Limb, LimbState>,
}

impl Baby {
    fn new(assets: &Assets, pos: vec2<f32>) -> Self {
        Self {
            pos,
            rotation: Angle::ZERO,
            head_rotation: Angle::ZERO,
            radius: assets.config.baby.radius,
            limbs: {
                let mut map = HashMap::new();
                for limb in Limb::all() {
                    map.insert(
                        limb,
                        LimbState {
                            rotation: Angle::ZERO,
                            angle: Angle::from_degrees(assets.config.baby.limbs[&limb].angle),
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
    framebuffer_size: vec2<f32>,
    prev_cursor_pos: vec2<f32>,
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
            framebuffer_size: vec2::splat(1.0),
            prev_cursor_pos: vec2::ZERO,
        }
    }

    fn draw_baby(&self, framebuffer: &mut ugli::Framebuffer, baby: &Baby) {
        let transform = mat3::translate(baby.pos)
            * mat3::rotate(baby.rotation)
            * mat3::scale_uniform(baby.radius);
        for limb in Limb::all() {
            let texture = match limb.is_leg() {
                true => &self.assets.baby.leg,
                false => &self.assets.baby.arm,
            };
            let config = &self.assets.config.baby.limbs[&limb];
            let limb = &baby.limbs[&limb];
            self.geng.draw2d().draw2d(
                framebuffer,
                &self.camera,
                &draw2d::TexturedQuad::unit(texture).transform(
                    transform
                        * mat3::translate(config.body_pos)
                        * mat3::rotate(limb.rotation)
                        * mat3::scale(vec2(if config.flip { -1.0 } else { 1.0 }, 1.0)),
                ),
            );
        }
        self.geng.draw2d().draw2d(
            framebuffer,
            &self.camera,
            &draw2d::TexturedQuad::unit(&self.assets.baby.body).transform(transform),
        );
        self.geng.draw2d().draw2d(
            framebuffer,
            &self.camera,
            &draw2d::TexturedQuad::unit(&self.assets.baby.head).transform(
                transform
                    * mat3::translate(self.assets.config.baby.head_offset)
                    * mat3::rotate(baby.head_rotation),
            ),
        );
    }

    fn baby_control(&mut self, cursor_pos: vec2<f32>) {
        let baby = &mut self.baby;
        baby.head_rotation = (((cursor_pos - (baby.pos + self.assets.config.baby.head_offset))
            .arg()
            - baby.rotation
            - Angle::from_degrees(90.0))
        .normalized_pi()
            * self.assets.config.baby.head_rotation_k)
            .clamp_abs(Angle::from_degrees(
                self.assets.config.baby.max_head_rotation,
            ));
        let delta = (cursor_pos - self.prev_cursor_pos) * self.assets.config.sensitivity;
        let air_control = self
            .geng
            .window()
            .is_button_pressed(geng::MouseButton::Right);
        let ground_control = self
            .geng
            .window()
            .is_button_pressed(geng::MouseButton::Left);
        if air_control || ground_control {
            let angle = (cursor_pos - baby.pos).arg();
            let limb = Limb::all()
                .min_by_key(|limb| {
                    (angle - baby.rotation - baby.limbs[limb].angle)
                        .normalized_pi()
                        .abs()
                        .map(r32)
                })
                .unwrap();
            let limb_config = &self.assets.config.baby.limbs[&limb];
            let limb = &mut baby.limbs.get_mut(&limb).unwrap();

            let old_body_pos = baby.pos + limb_config.body_pos.rotate(baby.rotation);
            let ground_pos = old_body_pos
                + limb_config
                    .touch_ground
                    .rotate(limb.rotation + baby.rotation);
            let new_body_pos = ground_pos
                + (old_body_pos - ground_pos - delta).normalize() * limb_config.touch_ground.len();
            limb.rotation =
                ((ground_pos - new_body_pos).arg() - limb.angle - baby.rotation).normalized_pi();
            limb.rotation = limb.rotation.clamp_abs(Angle::from_degrees(
                self.assets.config.baby.limb_rotation_limit,
            ));
            let new_body_pos = ground_pos
                - limb_config
                    .touch_ground
                    .rotate(limb.rotation + baby.rotation);
            if ground_control {
                let rotation = (new_body_pos - baby.pos).arg() - (old_body_pos - baby.pos).arg();
                baby.rotation += rotation;
                baby.pos += new_body_pos - (baby.pos + (old_body_pos - baby.pos).rotate(rotation));
            }
            // limb.rotation = angle - limb.angle;
        }
    }
}

impl geng::State for Game {
    fn draw(&mut self, framebuffer: &mut ugli::Framebuffer) {
        self.framebuffer_size = framebuffer.size().map(|x| x as f32);
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
        let cursor_window_pos = self.geng.window().cursor_position().unwrap_or(vec2::ZERO);
        let cursor_pos = self
            .camera
            .screen_to_world(self.framebuffer_size, cursor_window_pos.map(|x| x as f32));
        self.baby_control(cursor_pos);

        self.prev_cursor_pos = cursor_pos;
    }
}

fn main() {
    geng::setup_panic_handler();
    logger::init();
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
