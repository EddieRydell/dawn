use crate::model::{Time, TimeSpan, NANOS_PER_SECOND};

pub fn frame_start(frame: u64, frame_rate: u32) -> Time {
    if frame_rate == 0 {
        return Time::ZERO;
    }
    Time::from_nanoseconds(u128_to_u64_saturating(
        (frame as u128 * NANOS_PER_SECOND as u128) / frame_rate as u128,
    ))
}

pub fn floor_frame(time: Time, frame_rate: u32) -> u64 {
    if frame_rate == 0 {
        return 0;
    }
    u128_to_u64_saturating(
        (time.as_nanoseconds() as u128 * frame_rate as u128) / NANOS_PER_SECOND as u128,
    )
}

pub fn ceil_frame(time: Time, frame_rate: u32) -> u64 {
    if frame_rate == 0 || time.as_nanoseconds() == 0 {
        return 0;
    }
    u128_to_u64_saturating(
        (time.as_nanoseconds() as u128 * frame_rate as u128).div_ceil(NANOS_PER_SECOND as u128),
    )
}

pub fn nearest_frame(time: Time, frame_rate: u32) -> u64 {
    if frame_rate == 0 {
        return 0;
    }
    let numerator = time.as_nanoseconds() as u128 * frame_rate as u128;
    u128_to_u64_saturating((numerator + (NANOS_PER_SECOND as u128 / 2)) / NANOS_PER_SECOND as u128)
}

pub fn frame_count(span: TimeSpan, frame_rate: u32) -> usize {
    if frame_rate == 0 || span.as_nanoseconds() == 0 {
        return 0;
    }
    let frames =
        (span.as_nanoseconds() as u128 * frame_rate as u128).div_ceil(NANOS_PER_SECOND as u128);
    frames.min(usize::MAX as u128) as usize
}

fn u128_to_u64_saturating(value: u128) -> u64 {
    value.min(u64::MAX as u128) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sixty_fps_frame_3600_starts_at_sixty_seconds() {
        assert_eq!(
            frame_start(3600, 60),
            Time::from_nanoseconds(60 * NANOS_PER_SECOND)
        );
    }

    #[test]
    fn frame_starts_do_not_accumulate_rounded_duration_drift() {
        assert_eq!(frame_start(1, 60).as_nanoseconds(), 16_666_666);
        assert_eq!(frame_start(2, 60).as_nanoseconds(), 33_333_333);
        assert_eq!(frame_start(3, 60).as_nanoseconds(), 50_000_000);
    }

    #[test]
    fn frame_count_uses_ceil_for_partial_final_frame() {
        assert_eq!(frame_count(TimeSpan::from_nanoseconds(1), 60), 1);
        assert_eq!(
            frame_count(TimeSpan::from_nanoseconds(NANOS_PER_SECOND + 1), 60),
            61
        );
    }

    #[test]
    fn frame_rounding_helpers_are_explicit() {
        let time = Time::from_nanoseconds(24_900_000);
        assert_eq!(floor_frame(time, 60), 1);
        assert_eq!(nearest_frame(time, 60), 1);
        assert_eq!(ceil_frame(time, 60), 2);
    }
}
