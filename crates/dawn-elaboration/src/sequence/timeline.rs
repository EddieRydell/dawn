use crate::RenderError;
use dawn_language::sequence::Sequence;
use dawn_language::validation::{MAX_SEQUENCE_FRAME_COUNT, MAX_SEQUENCE_FRAME_RATE};
use dawn_language::values::{
    DawnDuration, DawnTime, NANOS_PER_SECOND, SampleTime, sample_time_from_dawn_time,
    sample_time_from_frame,
};

fn invalid_timing(reason: impl Into<String>) -> RenderError {
    RenderError::InvalidTiming {
        reason: reason.into(),
    }
}

pub(crate) fn prepare_timing(sequence: &Sequence) -> Result<(), RenderError> {
    if sequence.frame_rate == 0 {
        return Err(invalid_timing("frame rate must be greater than zero"));
    }
    if sequence.frame_rate > MAX_SEQUENCE_FRAME_RATE {
        return Err(invalid_timing(format!(
            "frame rate exceeds the limit of {MAX_SEQUENCE_FRAME_RATE} frames per second"
        )));
    }
    if sequence.duration.is_zero() {
        return Err(invalid_timing("duration must be positive"));
    }
    if frame_count(&sequence.duration, sequence.frame_rate)? > MAX_SEQUENCE_FRAME_COUNT {
        return Err(invalid_timing(format!(
            "sequence exceeds the prepared-frame budget of {MAX_SEQUENCE_FRAME_COUNT} frames"
        )));
    }
    sample_time_from_dawn_time(&DawnTime(sequence.duration.0))
        .map_err(|_| invalid_timing("sequence duration exceeds the runtime clock range"))?;
    Ok(())
}

pub(crate) fn frame_count(duration: &DawnDuration, frame_rate: u32) -> Result<u32, RenderError> {
    if frame_rate == 0 {
        return Err(invalid_timing("frame rate must be greater than zero"));
    }
    let scaled = duration.as_nanos() * u128::from(frame_rate);
    let frames = scaled.div_ceil(u128::from(NANOS_PER_SECOND));
    u32::try_from(frames).map_err(|_| invalid_timing("frame count exceeds the runtime range"))
}

pub(crate) fn sample_time_for_frame(
    frame_index: u32,
    frame_rate: u32,
) -> Result<SampleTime, RenderError> {
    sample_time_from_frame(frame_index, frame_rate)
        .map_err(|_| invalid_timing("frame time exceeds the runtime clock range"))
}

pub(crate) fn first_frame_at_or_after(time: SampleTime, frame_rate: u32) -> u32 {
    let whole = time.ticks() / dawn_language::values::MICROS_PER_SECOND;
    let partial = time.ticks() % dawn_language::values::MICROS_PER_SECOND;
    whole * frame_rate + (partial * frame_rate).div_ceil(dawn_language::values::MICROS_PER_SECOND)
}

pub(crate) fn frame_at_or_before(time: SampleTime, frame_rate: u32) -> u32 {
    let whole = time.ticks() / dawn_language::values::MICROS_PER_SECOND;
    let partial = time.ticks() % dawn_language::values::MICROS_PER_SECOND;
    whole * frame_rate + partial * frame_rate / dawn_language::values::MICROS_PER_SECOND
}
