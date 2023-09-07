use ambient_api::{core::transform::components::rotation, entity::get_component, prelude::*};
use packages::unit_schema::components::{
    jumping, run_direction, run_speed_multiplier, running, speed, strafe_speed_multiplier,
    vertical_velocity,
};

const FALLING_VSPEED: f32 = 0.4;

#[main]
pub fn main() {
    query((run_direction(), rotation(), vertical_velocity(), running())).each_frame(move |list| {
        for (unit_id, (direction, rot, vert_speed, running)) in list {
            let scale_factor = if running {
                get_component(unit_id, run_speed_multiplier()).unwrap_or(1.5)
            } else {
                1.
            } * get_component(unit_id, speed()).unwrap_or(0.06);
            let speed = scale_factor
                * vec2(
                    get_component(unit_id, strafe_speed_multiplier()).unwrap_or(0.8),
                    1.,
                );
            let displace = rot * (direction.normalize_or_zero() * speed).extend(vert_speed);
            let collision = physics::move_character(unit_id, displace, 0.01, delta_time());
            if collision.down {
                if let Some(is_jumping) = entity::get_component(unit_id, jumping()) {
                    if is_jumping {
                        entity::add_component(unit_id, jumping(), false);
                    }
                }

                entity::set_component(unit_id, vertical_velocity(), 0.0);
            } else {
                entity::mutate_component(unit_id, vertical_velocity(), |vertical_velocity| {
                    *vertical_velocity -= FALLING_VSPEED * delta_time(); // 1/60 second for example
                });
            }
        }
    });
}
