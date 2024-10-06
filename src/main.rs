#![allow(dead_code)]
use std::collections::BTreeMap;

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
    texture_origin: vec2<f32>,
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
struct OutlineConfig {
    scale: f32,
    color: Rgba<f32>,
    hovered_color: Rgba<f32>,
    ground_color: Rgba<f32>,
    ground_highlight_radius: f32,
    air_color: Rgba<f32>,
}

#[derive(Deserialize)]
struct UiConfig {
    edit_text_color: Rgba<f32>,
    timer_color: Rgba<f32>,
    timer_size: f32,
    fov: f32,
    label_color: Rgba<f32>,
    button_color: Rgba<f32>,
    hover_color: Rgba<f32>,
    text_offset: f32,
    rank_color: Rgba<f32>,
    rank_offset: f32,
    rank_size: f32,
}

#[derive(geng::asset::Load, Deserialize)]
#[load(serde = "toml")]
struct Config {
    nametag_offset: f32,
    nametag_color: Rgba<f32>,
    nametag_size: f32,
    outline: OutlineConfig,
    ui: UiConfig,
    background_color: Rgba<f32>,
    camera: CameraConfig,
    sensitivity: f32,
    baby: BabyConfig,
    ruler_color: Rgba<f32>,
    track_len: f32,
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
    #[load(options(filter = "ugli::Filter::Nearest", wrap_mode = "ugli::WrapMode::Repeat"))]
    ruler: ugli::Texture,
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
    fn all() -> impl Iterator<Item = Self> + Clone {
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
    spectating: bool,
    edit_name: bool,
    name_updated: bool,
    name: String,
    dbg: Option<vec2<f32>>,
    hovered_limb: Limb,
    locked_ground_pos: Option<vec2<f32>>,
    rank: Option<usize>,
    finish_time: f32,
    my_id: ClientId,
    geng: Geng,
    assets: Rc<Assets>,
    baby: Option<Baby>,
    host_race: bool,
    join_race: Option<ClientId>,
    others: BTreeMap<ClientId, ClientServerState>,
    camera: Camera2d,
    ui_camera: Camera2d,
    timer: Timer,
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
            spectating: false,
            name_updated: true,
            edit_name: false,
            name: preferences::load("name").unwrap_or("baby".to_owned()),
            finish_time: 0.0,
            hovered_limb: Limb::LeftArm,
            locked_ground_pos: None,
            rank: None,
            ui_camera: Camera2d {
                center: vec2::ZERO,
                rotation: Angle::ZERO,
                fov: Camera2dFov::MinSide(assets.config.ui.fov),
            },
            my_id,
            others: default(),
            join_race: None,
            host_race: false,
            connection,
            geng: geng.clone(),
            assets: assets.clone(),
            baby: None,
            camera: Camera2d {
                center: vec2::ZERO,
                rotation: Angle::ZERO,
                fov: Camera2dFov::MinSide(assets.config.camera.fov),
            },
            timer: Timer::new(),
            framebuffer_size: vec2::splat(1.0),
            prev_cursor_pos: vec2::ZERO,
            locked_limb: None,
            dbg: None,
        }
    }

    fn draw_baby(&self, framebuffer: &mut ugli::Framebuffer, baby: &Baby, me: bool) {
        let transform = mat3::translate(baby.pos)
            * mat3::rotate(baby.rotation)
            * mat3::scale_uniform(baby.radius);
        let parts = Limb::all()
            .map(|limb| {
                let texture = match limb.is_leg() {
                    true => &self.assets.baby.leg,
                    false => &self.assets.baby.arm,
                };
                let config = &self.assets.config.baby.limbs[&limb];
                let limb_state = &baby.limbs[&limb];
                (
                    texture,
                    transform
                        * mat3::translate(config.body_pos)
                        * mat3::rotate(limb_state.rotation)
                        * mat3::translate(-config.texture_origin)
                        * mat3::scale(vec2(if config.flip { -1.0 } else { 1.0 }, 1.0)),
                    Some(limb),
                )
            })
            .chain([
                (&self.assets.baby.body, transform, None),
                (
                    &self.assets.baby.head,
                    transform
                        * mat3::translate(self.assets.config.baby.head_offset)
                        * mat3::rotate(baby.head_rotation),
                    None,
                ),
            ]);
        if me {
            for (texture, transform, limb) in parts.clone() {
                let mut color = self.assets.config.outline.color;
                if let Some(limb) = limb {
                    if self.hovered_limb == limb {
                        color = self.assets.config.outline.hovered_color;
                    }
                    if self.locked_limb == Some(limb)
                        && self
                            .geng
                            .window()
                            .is_button_pressed(geng::MouseButton::Right)
                    {
                        color = self.assets.config.outline.air_color;
                    }
                }
                self.geng.draw2d().draw2d(
                    framebuffer,
                    &self.camera,
                    &draw2d::TexturedQuad::unit_colored(texture, color).transform(
                        transform * mat3::scale_uniform(self.assets.config.outline.scale),
                    ),
                );
            }
        }
        for (texture, transform, _limb) in parts {
            self.geng.draw2d().draw2d(
                framebuffer,
                &self.camera,
                &draw2d::TexturedQuad::unit(texture).transform(transform),
            );
        }
    }

    fn baby_control(&mut self, cursor_pos: vec2<f32>) {
        let Some(baby) = &mut self.baby else {
            self.locked_limb = None;
            self.locked_ground_pos = None;
            return;
        };
        if baby.pos.y < 0.0 {
            baby.pos.y = 0.0;
        }
        if baby.pos.y > self.assets.config.track_len {
            self.baby = None;
            self.connection.send(ClientMessage::Finish);
            return;
        }
        for (&id, other) in &self.others {
            if id == self.my_id {
                continue;
            }
            let Some(other) = &other.baby else {
                continue;
            };
            let delta_pos = other.pos - baby.pos;
            let penetration = baby.radius + other.radius - delta_pos.len();
            if delta_pos.len() > 1e-3 && penetration > 0.0 {
                baby.pos -= delta_pos.normalize() * penetration;
            }
        }
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
        let angle = (cursor_pos - baby.pos).arg();
        let hovered = Limb::all()
            .min_by_key(|limb| {
                (angle - baby.rotation - baby.limbs[limb].angle)
                    .normalized_pi()
                    .abs()
                    .map(r32)
            })
            .unwrap();
        if air_control || ground_control {
            let limb = match self.locked_limb {
                Some(limb) => limb,
                None => hovered,
            };
            self.hovered_limb = limb;
            self.locked_limb = Some(limb);
            let limb_config = &self.assets.config.baby.limbs[&limb];
            let limb = &mut baby.limbs.get_mut(&limb).unwrap();

            let old_body_pos = baby.pos + limb_config.body_pos.rotate(baby.rotation) * baby.radius;
            let ground_pos = self.locked_ground_pos.unwrap_or_else(|| {
                old_body_pos
                    + limb_config
                        .touch_ground
                        .rotate(limb.rotation + baby.rotation)
                        * baby.radius
            });
            // self.dbg = Some(old_body_pos);
            if ground_control {
                self.locked_ground_pos = Some(ground_pos);
            } else {
                self.locked_ground_pos = None;
            }
            // nothing looks correct here
            let new_body_pos = ground_pos
                + (old_body_pos - ground_pos - delta).normalize()
                    * limb_config.touch_ground.len()
                    * baby.radius;
            limb.rotation = ((ground_pos - new_body_pos).arg()
                - limb_config.touch_ground.arg()
                - baby.rotation)
                .normalized_pi();
            let rotation_limit = Angle::from_degrees(self.assets.config.baby.limb_rotation_limit);
            limb.rotation = limb.rotation.clamp_abs(rotation_limit);
            let new_body_pos = ground_pos
                - limb_config
                    .touch_ground
                    .rotate(limb.rotation + baby.rotation)
                    * baby.radius;
            if ground_control {
                let rotation = (new_body_pos - baby.pos).arg() - (old_body_pos - baby.pos).arg();
                let old_rotation = baby.rotation;
                let old_pos = baby.pos;
                baby.rotation += rotation;
                baby.pos += new_body_pos - (baby.pos + (old_body_pos - baby.pos).rotate(rotation));

                for limb in Limb::all() {
                    let limb_config = &self.assets.config.baby.limbs[&limb];
                    if Some(limb) == self.locked_limb {
                        continue;
                    }
                    let limb = &mut baby.limbs.get_mut(&limb).unwrap();
                    let old_body_pos =
                        old_pos + limb_config.body_pos.rotate(old_rotation) * baby.radius;
                    let old_ground_pos = old_body_pos
                        + limb_config
                            .touch_ground
                            .rotate(limb.rotation + baby.rotation)
                            * baby.radius;
                    let new_body_pos =
                        baby.pos + limb_config.body_pos.rotate(baby.rotation) * baby.radius;
                    limb.rotation += (new_body_pos - old_ground_pos).arg()
                        - (old_body_pos - old_ground_pos).arg();
                    limb.rotation = limb.rotation.normalized_pi().clamp_abs(rotation_limit);
                }
            }
            // limb.rotation = angle - limb.angle;
        } else {
            self.locked_limb = None;
            self.locked_ground_pos = None;
            self.hovered_limb = hovered;
        }
    }
    fn handler_multiplayer(&mut self) {
        let new_messages: Vec<_> = self.connection.new_messages().collect();
        for message in new_messages {
            let message = message.expect("server connection failure");
            match message {
                ServerMessage::Name(name) => self.name = name,
                ServerMessage::RaceResult { time, rank } => {
                    self.rank = Some(rank);
                    self.finish_time = time;
                }
                ServerMessage::Spawn(pos) => {
                    self.baby = Some(Baby::new(Some(&self.assets), pos));
                    self.host_race = false;
                    self.timer.reset();
                }
                ServerMessage::StateSync { clients } => {
                    self.others = clients
                        .into_iter()
                        .filter(|&(id, _)| id != self.my_id)
                        .collect();
                    self.connection.send(ClientMessage::StateSync(ClientState {
                        baby: self.baby.clone(),
                        host_race: self.host_race,
                        join_race: self.join_race,
                    }));
                }
                ServerMessage::Auth { .. } => unreachable!(),
            }
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Hash)]
enum MenuItemAction {
    StartRace,
    Host,
    Cancel,
    Join(ClientId),
    EditName,
    ToggleSpectating,
}

struct MenuItem {
    text: String,
    action: Option<MenuItemAction>,
}

impl Game {
    fn menu(&self) -> Vec<(f32, MenuItem)> {
        let mut items = vec![MenuItem {
            text: format!("your name: {}", self.name),
            action: Some(MenuItemAction::EditName),
        }];
        if self.host_race {
            items.extend([
                MenuItem {
                    text: "Start!".to_owned(),
                    action: Some(MenuItemAction::StartRace),
                },
                MenuItem {
                    text: "cancel".to_owned(),
                    action: Some(MenuItemAction::Cancel),
                },
                MenuItem {
                    text: "joined people:".to_owned(),
                    action: None,
                },
                MenuItem {
                    text: "YOU".to_owned(),
                    action: None,
                },
            ]);
            for (&id, client) in &self.others {
                if id == self.my_id {
                    continue;
                }
                if client.joined == Some(self.my_id) {
                    items.push(MenuItem {
                        text: client.name.clone(),
                        action: None,
                    });
                }
            }
        } else if let Some(joined) = self.join_race {
            items.extend([
                MenuItem {
                    text: "wait for the race to start".to_owned(),
                    action: None,
                },
                MenuItem {
                    text: "leave".to_owned(),
                    action: Some(MenuItemAction::Cancel),
                },
                MenuItem {
                    text: "joined people:".to_owned(),
                    action: None,
                },
                MenuItem {
                    text: "YOU".to_owned(),
                    action: None,
                },
            ]);
            for (&id, client) in &self.others {
                if id == self.my_id {
                    continue;
                }
                if client.joined == Some(joined) || id == joined {
                    items.push(MenuItem {
                        text: client.name.clone(),
                        action: None,
                    });
                }
            }
        } else {
            items.extend([
                MenuItem {
                    text: "Start SOLO!".to_owned(),
                    action: Some(MenuItemAction::StartRace),
                },
                MenuItem {
                    text: "Host a race".to_owned(),
                    action: Some(MenuItemAction::Host),
                },
                MenuItem {
                    text: "join race:".to_owned(),
                    action: None,
                },
            ]);
            for (&id, client) in &self.others {
                if id == self.my_id {
                    continue;
                }
                if client.hosting_race {
                    items.push(MenuItem {
                        text: client.name.clone(),
                        action: Some(MenuItemAction::Join(id)),
                    });
                }
            }
        }
        let mut result = Vec::new();
        let mut y = 0.0;
        for item in items {
            result.push((y, item));
            y -= 1.0;
        }
        let spectate = (
            self.assets.config.ui.fov / 2.0 - 1.0,
            MenuItem {
                text: if self.spectating {
                    "stop spectating"
                } else {
                    "spectate"
                }
                .to_owned(),
                action: Some(MenuItemAction::ToggleSpectating),
            },
        );
        if self.spectating {
            return vec![spectate];
        } else {
            result.push(spectate);
        }
        result
    }

    fn click_menu(&mut self) {
        if self.baby.is_some() {
            return;
        }
        let cursor = self.ui_camera.screen_to_world(
            self.framebuffer_size,
            self.geng
                .window()
                .cursor_position()
                .unwrap_or(vec2::ZERO)
                .map(|x| x as f32),
        );
        for (y, item) in self.menu() {
            let hovered = cursor.y > y && cursor.y < y + 1.0;
            if hovered {
                if let Some(action) = item.action {
                    self.perform_menu_action(action);
                    if !matches!(action, MenuItemAction::EditName) {
                        self.geng.window().stop_text_edit();
                    }
                    return;
                }
            }
        }
        self.geng.window().stop_text_edit();
    }

    fn perform_menu_action(&mut self, action: MenuItemAction) {
        match action {
            MenuItemAction::ToggleSpectating => self.spectating = !self.spectating,
            MenuItemAction::EditName => {
                self.edit_name = !self.edit_name;
                if self.edit_name {
                    self.geng.window().start_text_edit(&self.name);
                } else {
                    self.geng.window().stop_text_edit();
                }
            }
            MenuItemAction::StartRace => self.connection.send(ClientMessage::StartRace),
            MenuItemAction::Host => self.host_race = true,
            MenuItemAction::Cancel => {
                self.host_race = false;
                self.join_race = None;
            }
            MenuItemAction::Join(id) => self.join_race = Some(id),
        }
    }

    fn draw_menu(&self, framebuffer: &mut ugli::Framebuffer) {
        if self.baby.is_some() {
            return;
        }
        let _top_right_corner = self
            .ui_camera
            .view_area(self.framebuffer_size)
            .bounding_box()
            .top_right();
        let cursor = self.ui_camera.screen_to_world(
            self.framebuffer_size,
            self.geng
                .window()
                .cursor_position()
                .unwrap_or(vec2::ZERO)
                .map(|x| x as f32),
        );
        let font: &geng::Font = self.geng.default_font();

        if let Some(rank) = self.rank {
            let seconds = self.finish_time as i32;
            let minutes = seconds / 60;
            let seconds = seconds % 60;
            font.draw(
                framebuffer,
                &self.ui_camera,
                &format!("You placed #{rank} (time = {minutes}:{seconds:02})"),
                vec2(geng::TextAlign::CENTER, geng::TextAlign::BOTTOM),
                mat3::translate(vec2(0.0, self.assets.config.ui.rank_offset))
                    * mat3::scale_uniform(self.assets.config.ui.rank_size),
                self.assets.config.ui.rank_color,
            );
        }

        for (y, item) in self.menu() {
            let hovered = cursor.y > y && cursor.y < y + 1.0;
            if hovered && item.action.is_some() {
                self.geng.draw2d().quad(
                    framebuffer,
                    &self.ui_camera,
                    Aabb2::point(vec2(0.0, y))
                        .extend_up(1.0)
                        .extend_symmetric(vec2(self.assets.config.ui.fov * 2.0, 0.0)),
                    self.assets.config.ui.hover_color,
                );
            }
            if item.action == Some(MenuItemAction::EditName) && self.edit_name {
                self.geng.draw2d().quad(
                    framebuffer,
                    &self.ui_camera,
                    Aabb2::point(vec2(0.0, y))
                        .extend_up(1.0)
                        .extend_symmetric(vec2(self.assets.config.ui.fov * 2.0, 0.0)),
                    self.assets.config.ui.edit_text_color,
                );
                let w = font
                    .measure(
                        &item.text,
                        vec2(geng::TextAlign::CENTER, geng::TextAlign::BOTTOM),
                    )
                    .unwrap_or(Aabb2::ZERO);
                if self.timer.elapsed().as_secs_f64().fract() < 0.5 {
                    font.draw(
                        framebuffer,
                        &self.ui_camera,
                        "|",
                        vec2(geng::TextAlign::CENTER, geng::TextAlign::BOTTOM),
                        mat3::translate(vec2(w.max.x, y + self.assets.config.ui.text_offset)),
                        match item.action {
                            None => self.assets.config.ui.label_color,
                            Some(_) => self.assets.config.ui.button_color,
                        },
                    );
                }
            }
            font.draw(
                framebuffer,
                &self.ui_camera,
                &item.text,
                vec2(geng::TextAlign::CENTER, geng::TextAlign::BOTTOM),
                mat3::translate(vec2(0.0, y + self.assets.config.ui.text_offset)),
                match item.action {
                    None => self.assets.config.ui.label_color,
                    Some(_) => self.assets.config.ui.button_color,
                },
            );
        }
    }
}

impl geng::State for Game {
    fn handle_event(&mut self, event: geng::Event) {
        match event {
            geng::Event::KeyPress { key } => {
                if key == geng::Key::R {
                    self.baby = None;
                    self.connection.send(ClientMessage::Despawn);
                }
                if key == geng::Key::Enter
                    || key == geng::Key::NumpadEnter
                    || key == geng::Key::Escape
                {
                    self.geng.window().stop_text_edit();
                }
            }
            geng::Event::MousePress {
                button: geng::MouseButton::Left,
            } => {
                self.click_menu();
            }
            geng::Event::EditText(new_text) => {
                if self.edit_name {
                    self.name = new_text;
                } else {
                    self.geng.window().stop_text_edit();
                }
            }
            _ => (),
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
        self.geng.draw2d().draw_textured(
            framebuffer,
            &self.camera,
            &[(-1, 0), (1, 0), (1, 1), (-1, 1)].map(|(x, y)| {
                let world_x = self.camera.center.x + x as f32 * self.assets.config.camera.fov * 2.0;
                draw2d::TexturedVertex {
                    a_pos: vec2(world_x, y as f32 * self.assets.config.track_len),
                    a_color: Rgba::WHITE,
                    a_vt: vec2(
                        world_x
                            / self.assets.config.track_len
                            / self.assets.ruler.size().map(|x| x as f32).aspect(),
                        y as f32,
                    ),
                }
            }),
            &self.assets.ruler,
            self.assets.config.ruler_color,
            ugli::DrawMode::TriangleFan,
        );
        for other in self.others.values() {
            if let Some(baby) = &other.baby {
                self.draw_baby(framebuffer, baby, false);
                self.geng.default_font().draw(
                    framebuffer,
                    &self.camera,
                    &other.name,
                    vec2(geng::TextAlign::CENTER, geng::TextAlign::BOTTOM),
                    mat3::translate(baby.pos + vec2(0.0, self.assets.config.nametag_offset))
                        * mat3::scale_uniform(self.assets.config.nametag_size),
                    self.assets.config.nametag_color,
                );
            }
        }
        if let Some(baby) = &self.baby {
            self.draw_baby(framebuffer, baby, true);
            if let Some(pos) = self.locked_ground_pos {
                self.geng.draw2d().circle(
                    framebuffer,
                    &self.camera,
                    pos,
                    self.assets.config.outline.ground_highlight_radius,
                    self.assets.config.outline.ground_color,
                );
            }
            let seconds = self.timer.elapsed().as_secs_f64() as i32;
            let minutes = seconds / 60;
            let seconds = seconds % 60;
            self.geng.default_font().draw(
                framebuffer,
                &self.ui_camera,
                &format!("{minutes}:{seconds:02}"),
                vec2(geng::TextAlign::CENTER, geng::TextAlign::TOP),
                mat3::translate(vec2(0.0, self.assets.config.ui.fov / 2.0))
                    * mat3::scale_uniform(self.assets.config.ui.timer_size),
                self.assets.config.ui.timer_color,
            );
        }
        if let Some(pos) = self.dbg {
            self.geng.draw2d().circle(
                framebuffer,
                &self.camera,
                pos,
                self.assets.config.outline.ground_highlight_radius,
                self.assets.config.outline.ground_color,
            );
        }
        self.draw_menu(framebuffer);
    }
    fn update(&mut self, delta_time: f64) {
        if !self.geng.window().is_editing_text() && self.edit_name {
            self.edit_name = false;
            self.name_updated = true;
            preferences::save("name", &self.name);
        }
        if self.name_updated {
            self.name_updated = false;
            self.connection.send(ClientMessage::Name(self.name.clone()));
        }
        if let Some(joined) = self.join_race {
            if self.baby.is_none()
                && !self
                    .others
                    .get(&joined)
                    .map_or(false, |host| host.hosting_race)
            {
                self.join_race = None;
            }
        }
        self.handler_multiplayer();
        let delta_time = delta_time as f32;
        let cursor_window_pos = self.geng.window().cursor_position().unwrap_or(vec2::ZERO);
        let cursor_pos = self
            .camera
            .screen_to_world(self.framebuffer_size, cursor_window_pos.map(|x| x as f32));
        self.baby_control(cursor_pos);
        let target_pos = if let Some(baby) = &mut self.baby {
            baby.pos
        } else {
            self.others
                .values()
                .filter_map(|other| other.baby.as_ref())
                .map(|baby| baby.pos)
                .max_by_key(|pos| r32(pos.y))
                .unwrap_or(vec2::ZERO)
        };
        self.camera.center += (target_pos - self.camera.center)
            * (delta_time * self.assets.config.camera.speed).min(1.0);

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
