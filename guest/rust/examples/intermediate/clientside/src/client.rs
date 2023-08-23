use ambient_api::{
    core::{
        app::components::main_scene,
        camera::{
            components::aspect_ratio_from_window,
            concepts::make_perspective_infinite_reverse_camera,
        },
        messages::Frame,
        primitives::components::cube,
        rendering::components::color,
        transform::components::{lookat_target, translation},
    },
    prelude::*,
};
use packages::this::components::{grid_position, grid_side_length};

#[main]
pub async fn main() {
    let side_length =
        entity::wait_for_component(entity::synchronized_resources(), grid_side_length())
            .await
            .unwrap();

    let id = Entity::new()
        .with_merge(make_perspective_infinite_reverse_camera())
        .with(aspect_ratio_from_window(), EntityId::resources())
        .with(main_scene(), ())
        .with(translation(), Vec3::ONE * 5.)
        .with(lookat_target(), vec3(0., 0., 0.))
        .spawn();

    let start_time = game_time();

    Frame::subscribe(move |_| {
        let t = game_time() - start_time;
        entity::set_component(
            id,
            translation(),
            Quat::from_rotation_z(t.as_secs_f32() * 0.2) * Vec3::ONE * 10.,
        );
    });

    query(grid_position())
        .requires(cube())
        .each_frame(move |entities| {
            for (id, position) in entities {
                let [x, y] = position.to_array();
                let grid_cell = position - glam::ivec2(side_length, side_length);
                let t = (game_time() - start_time).as_secs_f32();
                entity::mutate_component(id, translation(), |v| {
                    v.z = (x as f32 + y as f32 + t).sin() - 0.5 * grid_cell.as_vec2().length();
                });

                let s = (t.sin() + 1.0) / 2.0;
                let t = (((x + y) as f32).sin() + 1.0) / 2.0;
                entity::set_component(id, color(), vec3(s, 1.0 - s, t).extend(1.0));
            }
        });
}
