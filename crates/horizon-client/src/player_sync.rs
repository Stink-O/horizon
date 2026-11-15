//! LocalPlayer.sendPosition and PositionMoveRotation.calculateAbsolute.
use horizon_core::geometry::Vec3;
use horizon_protocol::packets::play::{self, PlayerPosition, Serverbound as S, relative};

/// Only movement updates advance this baseline. A teleport acknowledgement is
/// separate and must not suppress the next tick's corrected position or look.
#[derive(Default)]
pub struct PositionSender {
    position: [f64; 3],
    rotation: (f32, f32),
    on_ground: bool,
    collision: bool,
    reminder: u32,
}

impl PositionSender {
    pub fn update(
        &mut self,
        position: [f64; 3],
        rotation: (f32, f32),
        on_ground: bool,
        collision: bool,
    ) -> Option<S> {
        self.reminder += 1;
        let d: [f64; 3] = std::array::from_fn(|i| position[i] - self.position[i]);
        let moved =
            d[0] * d[0] + d[1] * d[1] + d[2] * d[2] > 2.0e-4 * 2.0e-4 || self.reminder >= 20;
        let rotated = rotation.0 - self.rotation.0 != 0.0 || rotation.1 - self.rotation.1 != 0.0;
        let flags = u8::from(on_ground) | u8::from(collision) << 1;
        let [x, y, z] = position;
        let (yaw, pitch) = rotation;
        let packet = if moved && rotated {
            Some(S::MovePlayerPosRot(play::MovePlayerPosRot {
                x,
                y,
                z,
                yaw,
                pitch,
                flags,
            }))
        } else if moved {
            Some(S::MovePlayerPos(play::MovePlayerPos { x, y, z, flags }))
        } else if rotated {
            Some(S::MovePlayerRot(play::MovePlayerRot { yaw, pitch, flags }))
        } else if self.on_ground != on_ground || self.collision != collision {
            Some(S::MovePlayerStatus(play::MovePlayerStatus { flags }))
        } else {
            None
        };
        if moved {
            self.position = position;
            self.reminder = 0;
        }
        if rotated {
            self.rotation = rotation;
        }
        self.on_ground = on_ground;
        self.collision = collision;
        packet
    }
}

/// Rotate the old velocity before adding the relative delta components.
/// Angles deliberately use Java's double toRadians followed by a float cast.
pub fn teleport_velocity(
    mut velocity: Vec3,
    old_rotation: (f32, f32),
    new_rotation: (f32, f32),
    packet: &PlayerPosition,
) -> Vec3 {
    if packet.relatives & relative::ROTATE_DELTA != 0 {
        let pitch = f64::from(old_rotation.1 - new_rotation.1).to_radians() as f32;
        let yaw = f64::from(old_rotation.0 - new_rotation.0).to_radians() as f32;
        velocity = velocity.rotate_x(pitch).rotate_y(yaw);
    }
    let old = [velocity.x, velocity.y, velocity.z];
    let flags = [relative::DELTA_X, relative::DELTA_Y, relative::DELTA_Z];
    let result: [f64; 3] = std::array::from_fn(|i| {
        if packet.relatives & flags[i] != 0 {
            old[i] + packet.delta[i]
        } else {
            packet.delta[i]
        }
    });
    Vec3::new(result[0], result[1], result[2])
}

/// The channel of the custom payload a single-player window sends its own server thread before the move that follows a
/// mod's `world.shift-player`: the server moves its copy of the owner by the same amount first, so that move is the
/// small one it is after the shift (no "moved wrongly" for what the shift passed through, nothing sent back).
pub const SHIFT_CHANNEL: &str = "horizon:shift_player";

/// The shift's payload: x, y, z as big-endian doubles.
pub fn shift_payload(delta: [f64; 3]) -> Vec<u8> {
    delta.iter().flat_map(|c| c.to_be_bytes()).collect()
}

/// A shift's payload read back: none unless it is three finite doubles, each within `limit`.
pub fn read_shift(data: &[u8], limit: f64) -> Option<[f64; 3]> {
    if data.len() != 24 {
        return None;
    }
    let d: [f64; 3] = std::array::from_fn(|k| f64::from_be_bytes(data[k * 8..k * 8 + 8].try_into().unwrap()));
    d.iter().all(|c| c.is_finite() && c.abs() <= limit).then_some(d)
}

/// `world.shift-player` on the window's player: `position` and the position the frames interpolate from (`previous`)
/// both moved by `delta`, so every frame between two ticks draws them `delta` further on and none draws them sliding.
pub fn shift(position: &mut Vec3, previous: &mut Vec3, delta: [f64; 3]) {
    let d = Vec3::new(delta[0], delta[1], delta[2]);
    *position = position.add_vector(d);
    *previous = previous.add_vector(d);
}

#[cfg(test)]
mod shift_tests {
    use super::*;

    #[test]
    fn a_shift_is_not_interpolated() {
        let (mut at, mut was) = (Vec3::new(10.0, 64.0, -3.0), Vec3::new(9.8, 64.1, -3.0));
        let lerp = |a: Vec3, b: Vec3, t: f64| Vec3::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t, a.z + (b.z - a.z) * t);
        let before: Vec<Vec3> = (0..=10).map(|n| lerp(was, at, f64::from(n) / 10.0)).collect();
        shift(&mut at, &mut was, [1.5, -38.0, 0.25]);
        for (n, b) in before.iter().enumerate() {
            let now = lerp(was, at, n as f64 / 10.0);
            assert!((now.x - b.x - 1.5).abs() < 1e-9 && (now.y - b.y + 38.0).abs() < 1e-9 && (now.z - b.z - 0.25).abs() < 1e-9);
        }
    }

    #[test]
    fn a_shift_payload_reads_back_and_refuses_junk() {
        let d = [1.5, -38.0, 1e-3];
        assert_eq!(read_shift(&shift_payload(d), 512.0), Some(d));
        assert_eq!(read_shift(&shift_payload([f64::NAN, 0.0, 0.0]), 512.0), None);
        assert_eq!(read_shift(&shift_payload([600.0, 0.0, 0.0]), 512.0), None);
        assert_eq!(read_shift(&[0; 23], 512.0), None);
    }
}
