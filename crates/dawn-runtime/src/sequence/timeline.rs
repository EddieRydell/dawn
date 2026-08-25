use crate::{PreparedEffect, RenderError};
use dawn_language::sequence::Sequence;
use dawn_language::validation::MAX_SEQUENCE_FRAME_COUNT;

pub(crate) fn prepare_timing(sequence: &Sequence) -> Result<(), RenderError> {
    if sequence.frame_rate == 0 {
        return Err(RenderError::InvalidTiming {
            reason: "frame rate must be greater than zero".to_string(),
        });
    }
    let duration_seconds = sequence.duration.as_seconds_f64();
    if !duration_seconds.is_finite() || duration_seconds <= 0.0 {
        return Err(RenderError::InvalidTiming {
            reason: "duration must be positive and finite".to_string(),
        });
    }
    let prepared_frames = duration_seconds * f64::from(sequence.frame_rate);
    if !prepared_frames.is_finite() || prepared_frames.ceil() > MAX_SEQUENCE_FRAME_COUNT as f64 {
        return Err(RenderError::InvalidTiming {
            reason: format!(
                "sequence exceeds the prepared-frame budget of {MAX_SEQUENCE_FRAME_COUNT} frames"
            ),
        });
    }
    Ok(())
}

pub(crate) fn frame_count(duration_seconds: f64, frame_rate: u32) -> u64 {
    (duration_seconds * f64::from(frame_rate)).ceil() as u64
}

pub(crate) fn build_effect_frame_index(
    effects: &[PreparedEffect],
    frame_count: u64,
    frame_rate: u32,
) -> Vec<Vec<usize>> {
    build_effect_frame_index_for_window(effects, 0, frame_count, frame_rate)
}

pub(crate) fn build_effect_frame_index_for_window(
    effects: &[PreparedEffect],
    start_frame: u64,
    frame_count: u64,
    frame_rate: u32,
) -> Vec<Vec<usize>> {
    let end_frame_limit = start_frame.saturating_add(frame_count);
    let effect_frame_range = |effect: &PreparedEffect| {
        let effect_start_frame = (effect.start_seconds * f64::from(frame_rate))
            .floor()
            .max(0.0) as u64;
        let effect_end_frame = ((effect.start_seconds + effect.duration_seconds)
            * f64::from(frame_rate))
        .ceil() as u64;
        effect_start_frame.max(start_frame)..effect_end_frame.min(end_frame_limit)
    };
    let mut active_counts = vec![0usize; frame_count as usize];
    for effect in effects {
        for frame in effect_frame_range(effect) {
            active_counts[frame.saturating_sub(start_frame) as usize] += 1;
        }
    }
    let mut index = active_counts
        .into_iter()
        .map(Vec::with_capacity)
        .collect::<Vec<_>>();
    for (effect_index, effect) in effects.iter().enumerate() {
        for frame in effect_frame_range(effect) {
            index[frame.saturating_sub(start_frame) as usize].push(effect_index);
        }
    }
    index
}
