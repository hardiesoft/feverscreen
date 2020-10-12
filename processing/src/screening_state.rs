use crate::init::{SCREENING_STATE, STATE_MAP};
use crate::{point_is_in_triangle, FaceInfo, HeadLockConfidence, Rect};
#[allow(unused)]
use log::{info, trace, warn};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum ScreeningState {
    WarmingUp,
    Ready,
    HeadLock,
    TooFar,
    HasBody,
    FaceLock,
    FrontalLock,
    StableLock,
    Measured,
    MissingThermalRef,
}

#[derive(Copy, Clone)]
pub struct ScreeningValue {
    pub state: ScreeningState,
    pub count: u32,
}

pub fn get_current_state() -> ScreeningValue {
    SCREENING_STATE.with(|prev| prev.get())
}

pub fn advance_screening_state(next: ScreeningState) {
    SCREENING_STATE.with(|prev| {
        let prev_val = prev.get();
        if prev_val.state != next {
            if let Some(allowed_next_states) = STATE_MAP.get(&prev_val.state) {
                if allowed_next_states.contains(&next) {
                    prev.set(ScreeningValue {
                        state: next,
                        count: 1,
                    });
                }
            }
        } else {
            prev.set(ScreeningValue {
                state: prev_val.state,
                count: prev_val.count + 1,
            });
        }
    });
}

fn demote_current_state() {
    SCREENING_STATE.with(|state| {
        let mut curr = state.get();
        curr.count = i32::max(curr.count as i32 - 2, 0) as u32;
        state.set(curr);
    });
}

fn face_is_too_small(face: &FaceInfo) -> bool {
    let width = face.head.top_left.distance_to(face.head.top_right);

    if width > 30.0 {
        return false;
    } else {
        let prev_state = get_current_state();
        if prev_state.state != ScreeningState::TooFar && width + 3.0 > 30.0 {
            // Don't flip-flop between too far and close enough.
            return false;
        }
        face.head.area() < 1200.0
    }
}

fn face_intersects_thermal_ref(face: &FaceInfo, thermal_ref_rect: Rect) -> bool {
    for p in [
        thermal_ref_rect.top_left(),
        thermal_ref_rect.top_right(),
        thermal_ref_rect.bottom_left(),
        thermal_ref_rect.bottom_right(),
    ]
    .iter()
    {
        if point_is_in_triangle(
            *p,
            face.head.top_left,
            face.head.top_right,
            face.head.bottom_left,
        ) || point_is_in_triangle(
            *p,
            face.head.top_left,
            face.head.top_right,
            face.head.bottom_right,
        ) {
            return true;
        }
    }
    return false;
}

fn face_is_front_on(face: &FaceInfo) -> bool {
    // TODO(jon): This needs to be better, we are already checking for this case.
    face.head_lock != HeadLockConfidence::Bad
}

fn face_has_moved_or_changed_in_size(face: &FaceInfo, prev_face: &Option<FaceInfo>) -> bool {
    match prev_face {
        Some(prev_face) => {
            let prev_area = prev_face.head.area();
            let next_area = face.head.area();
            let diff_area = f32::abs(next_area - prev_area);
            let ten_percent_of_area = next_area / 10.0;
            if diff_area > ten_percent_of_area {
                return true;
            }
            [
                face.head.top_left.distance_to(prev_face.head.top_left),
                face.head
                    .bottom_left
                    .distance_to(prev_face.head.bottom_left),
                face.head
                    .bottom_right
                    .distance_to(prev_face.head.bottom_right),
                face.head.top_right.distance_to(prev_face.head.top_right),
            ]
            .iter()
            .filter(|&d| *d > 10.0)
            .count()
                != 0
        }
        None => true,
    }
}

fn advance_state_with_face(face: FaceInfo, prev_face: Option<FaceInfo>, thermal_ref_rect: Rect) {
    if face_is_too_small(&face) {
        advance_screening_state(ScreeningState::TooFar);
    } else if face_intersects_thermal_ref(&face, thermal_ref_rect) {
        advance_screening_state(ScreeningState::HasBody)
    } else if face.head_lock != HeadLockConfidence::Bad {
        if face_is_front_on(&face) {
            if !face_has_moved_or_changed_in_size(&face, &prev_face) {
                let current_state = get_current_state();
                if current_state.state == ScreeningState::FrontalLock && current_state.count >= 2 {
                    advance_screening_state(ScreeningState::StableLock);
                } else if current_state.state == ScreeningState::StableLock {
                    advance_screening_state(ScreeningState::Measured);
                } else {
                    advance_screening_state(ScreeningState::FrontalLock);
                }
            } else {
                advance_screening_state(ScreeningState::FrontalLock);
                demote_current_state();
            }
        } else {
            advance_screening_state(ScreeningState::FaceLock);
        }
    } else {
        advance_screening_state(ScreeningState::HeadLock);
    }
}

fn advance_state_without_face(has_body: bool, prev_frame_has_body: bool) {
    if has_body || prev_frame_has_body {
        advance_screening_state(ScreeningState::HasBody);
    } else {
        let current_state = get_current_state();
        if current_state.state == ScreeningState::Measured && current_state.count == 0 {
            advance_screening_state(ScreeningState::Measured);
        } else {
            advance_screening_state(ScreeningState::Ready);
        }
    }
}

pub fn advance_state(
    face: Option<FaceInfo>,
    prev_face: Option<FaceInfo>,
    thermal_ref_rect: Option<Rect>,
    has_body: bool,
    prev_frame_has_body: bool,
) {
    match thermal_ref_rect {
        Some(thermal_ref_rect) => match face {
            Some(face) => advance_state_with_face(face, prev_face, thermal_ref_rect),
            None => advance_state_without_face(has_body, prev_frame_has_body),
        },
        None => advance_screening_state(ScreeningState::MissingThermalRef),
    }
}