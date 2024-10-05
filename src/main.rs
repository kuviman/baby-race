#![allow(dead_code)]
use geng::prelude::*;

mod interop;
#[cfg(not(target_arch = "wasm32"))]
mod server;

use interop::*;

#[derive(clap::Parser)]
struct CliArgs {
    #[clap(long)]
    pub server: Option<String>,
    #[clap(long)]
    pub connect: Option<String>,
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

#[derive(Deserialize)]
struct CameraConfig {
    fov: f32,
    speed: f32,
}

#[derive(Deserialize)]
struct InfoConfig {
    tiny_scale: f32,
    timer_size: f32,
    timer_color: Rgba<f32>,
    join_offset: vec2<f32>,
    join_size: f32,
    join_color: Rgba<f32>,
}

#[derive(geng::asset::Load, Deserialize)]
#[load(serde = "toml")]
struct Config {
    info: InfoConfig,
    background_color: Rgba<f32>,
    camera: CameraConfig,
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

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LimbState {
    rotation: Angle<f32>,
    angle: Angle<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Baby {
    pos: vec2<f32>,
    rotation: Angle<f32>,
    head_rotation: Angle<f32>,
    radius: f32,
    limbs: HashMap<Limb, LimbState>,
}

impl Baby {
    fn new(assets: Option<&Assets>, pos: vec2<f32>) -> Self {
        Self {
            pos,
            rotation: Angle::ZERO,
            head_rotation: Angle::ZERO,
            radius: assets.map_or(1.0, |assets| assets.config.baby.radius),
            limbs: {
                let mut map = HashMap::new();
                for limb in Limb::all() {
                    map.insert(
                        limb,
                        LimbState {
                            rotation: Angle::ZERO,
                            angle: Angle::from_degrees(
                                assets.map_or(0.0, |assets| assets.config.baby.limbs[&limb].angle),
                            ),
                        },
                    );
                }
                map
            },
        }
    }
}

struct Game {
    my_id: ClientId,
    until_next_race: f32,
    joining_next_race: usize,
    join_next: bool,
    geng: Geng,
    assets: Rc<Assets>,
    baby: Option<Baby>,
    other_babies: HashMap<ClientId, Baby>,
    camera: Camera2d,
    time: f32,
    framebuffer_size: vec2<f32>,
    prev_cursor_pos: vec2<f32>,
    connection: Connection,
    locked_limb: Option<Limb>,
}

type Connection = geng::net::client::Connection<ServerMessage, ClientMessage>;

impl Game {
    pub async fn new(geng: &Geng, assets: &Rc<Assets>, mut connection: Connection) -> Self {
        let ServerMessage::Auth { id: my_id } = connection.next().await.unwrap().unwrap() else {
            unreachable!()
        };
        Self {
            until_next_race: 0.0,
            joining_next_race: 0,
            my_id,
            join_next: false,
            connection,
            geng: geng.clone(),
            assets: assets.clone(),
            baby: None,
            other_babies: HashMap::new(),
            camera: Camera2d {
                center: vec2::ZERO,
                rotation: Angle::ZERO,
                fov: Camera2dFov::MinSide(assets.config.camera.fov),
            },
            time: 0.0,
            framebuffer_size: vec2::splat(1.0),
            prev_cursor_pos: vec2::ZERO,
            locked_limb: None,
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
        let Some(baby) = &mut self.baby else { return };
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
            let limb = match self.locked_limb {
                Some(limb) => limb,
                None => Limb::all()
                    .min_by_key(|limb| {
                        (angle - baby.rotation - baby.limbs[limb].angle)
                            .normalized_pi()
                            .abs()
                            .map(r32)
                    })
                    .unwrap(),
            };
            self.locked_limb = Some(limb);
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
        } else {
            self.locked_limb = None;
        }
    }
    fn handler_multiplayer(&mut self) {
        let new_messages: Vec<_> = self.connection.new_messages().collect();
        for message in new_messages {
            let message = message.expect("server connection failure");
            match message {
                ServerMessage::Spawn(pos) => {
                    self.baby = Some(Baby::new(Some(&self.assets), pos));
                    self.join_next = false;
                }
                ServerMessage::StateSync {
                    babies,
                    until_next_race,
                    joining_next_race,
                } => {
                    self.until_next_race = until_next_race;
                    self.joining_next_race = joining_next_race;
                    self.other_babies = babies
                        .into_iter()
                        .filter(|&(id, _)| id != self.my_id)
                        .collect();
                    self.connection.send(ClientMessage::StateSync(ClientState {
                        baby: self.baby.clone(),
                        join_next: self.join_next,
                    }))
                }
                ServerMessage::Auth { .. } => unreachable!(),
            }
        }
    }

    fn show_info(&self, framebuffer: &mut ugli::Framebuffer) {
        if self.baby.is_some() {
            return;
        }
        let top_right_corner = self
            .camera
            .view_area(self.framebuffer_size)
            .bounding_box()
            .top_right();
        let next_race_tiny = self.baby.is_some();
        let info_tranform = if next_race_tiny {
            mat3::scale_uniform_around(top_right_corner, self.assets.config.info.tiny_scale)
        } else {
            mat3::identity()
        };
        let font: &geng::Font = self.geng.default_font();
        font.draw(
            framebuffer,
            &self.camera,
            &format!("until next race: {} seconds", self.until_next_race as i32),
            vec2::splat(geng::TextAlign::CENTER),
            info_tranform
                * mat3::translate(self.camera.center)
                * mat3::scale_uniform(self.assets.config.info.timer_size),
            self.assets.config.info.timer_color,
        );
        let text = if self.join_next {
            "you have joined, just wait"
        } else {
            "press SPACE to join"
        };
        font.draw(
            framebuffer,
            &self.camera,
            text,
            vec2::splat(geng::TextAlign::CENTER),
            info_tranform
                * mat3::translate(self.camera.center + self.assets.config.info.join_offset)
                * mat3::scale_uniform(self.assets.config.info.join_size),
            self.assets.config.info.join_color,
        );
    }
}

impl geng::State for Game {
    fn handle_event(&mut self, event: geng::Event) {
        if let geng::Event::KeyPress { key } = event {
            match key {
                geng::Key::Space => {
                    if self.baby.is_none() {
                        self.join_next = true;
                    }
                }
                geng::Key::R => {
                    self.baby = None;
                    self.connection.send(ClientMessage::Despawn);
                }
                _ => {}
            }
        }
    }
    fn draw(&mut self, framebuffer: &mut ugli::Framebuffer) {
        self.framebuffer_size = framebuffer.size().map(|x| x as f32);
        ugli::clear(
            framebuffer,
            Some(self.assets.config.background_color),
            None,
            None,
        );
        for baby in self.other_babies.values() {
            self.draw_baby(framebuffer, baby);
        }
        if let Some(baby) = &self.baby {
            self.draw_baby(framebuffer, baby);
        }
        self.show_info(framebuffer);
    }
    fn update(&mut self, delta_time: f64) {
        self.handler_multiplayer();
        let delta_time = delta_time as f32;
        self.time += delta_time;
        let cursor_window_pos = self.geng.window().cursor_position().unwrap_or(vec2::ZERO);
        let cursor_pos = self
            .camera
            .screen_to_world(self.framebuffer_size, cursor_window_pos.map(|x| x as f32));
        self.baby_control(cursor_pos);
        if let Some(baby) = &self.baby {
            self.camera.center += (baby.pos - self.camera.center)
                * (delta_time * self.assets.config.camera.speed).min(1.0);
        }

        self.prev_cursor_pos = cursor_pos;
    }
}

fn main() {
    geng::setup_panic_handler();
    logger::init();
    let mut cli_args: CliArgs = cli::parse();
    if cli_args.connect.is_none() && cli_args.server.is_none() {
        #[cfg(target_arch = "wasm32")]
        {
            cli_args.connect = Some(
                option_env!("CONNECT")
                    .filter(|addr| !addr.is_empty())
                    .map(|addr| addr.to_owned())
                    .unwrap_or_else(|| {
                        let window = web_sys::window().unwrap();
                        let location = window.location();
                        let mut new_uri = String::new();
                        if location.protocol().unwrap() == "https" {
                            new_uri += "wss://";
                        } else {
                            new_uri += "ws://";
                        }
                        new_uri += &location.host().unwrap();
                        new_uri += &location.pathname().unwrap();
                        new_uri
                    }),
            );
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            cli_args.server = Some("127.0.0.1:1155".to_owned());
            cli_args.connect = Some("ws://127.0.0.1:1155".to_owned());
        }
    }

    if cli_args.server.is_some() && cli_args.connect.is_none() {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let server =
                geng::net::Server::new(server::App::new(), cli_args.server.as_deref().unwrap());
            let server_handle = server.handle();
            ctrlc::set_handler(move || server_handle.shutdown()).unwrap();
            server.run();
        }
    } else {
        #[cfg(not(target_arch = "wasm32"))]
        let server = if let Some(addr) = &cli_args.server {
            let server = geng::net::Server::new(server::App::new(), addr);
            let server_handle = server.handle();
            let server_thread = std::thread::spawn(move || {
                server.run();
            });
            Some((server_handle, server_thread))
        } else {
            None
        };

        let mut geng_options = geng::ContextOptions::default();
        geng_options.with_cli(&cli_args.geng);
        Geng::run_with(&geng_options, move |geng| async move {
            let connection = geng::net::client::connect(&cli_args.connect.unwrap())
                .await
                .unwrap();
            let assets = geng
                .asset_manager()
                .load(run_dir().join("assets"))
                .await
                .expect("failed to load assets");
            geng.run_state(Game::new(&geng, &assets, connection).await)
                .await
        });

        #[cfg(not(target_arch = "wasm32"))]
        if let Some((server_handle, server_thread)) = server {
            server_handle.shutdown();
            server_thread.join().unwrap();
        }
    }
}
