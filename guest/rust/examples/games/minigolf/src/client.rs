use ambient_api::{player::MouseButton, prelude::*};

#[main]
fn main() {
    ambient_api::messages::Frame::subscribe(move |_, _| {
        let (delta, input) = player::get_raw_input_delta();

        let camera_rotation = if input.mouse_buttons.contains(&MouseButton::Right) {
            delta.mouse_position
        } else {
            Vec2::ZERO
        };

        let camera_zoom = delta.mouse_wheel;
        let shoot = delta.mouse_buttons.contains(&MouseButton::Left);

        messages::Input::new(camera_rotation, camera_zoom, shoot).send_server_unreliable();
    });
}
