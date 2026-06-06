use crate::model::{Color, Curve, CurveValue};

use super::bytecode::{
    stats_for_program, BinaryInstruction, BytecodeProgram, ContextSlot, FloatSlot, Instruction,
    MarkSearchInstruction, RefSlot, UnaryFloatInstruction, ValueSlot,
};
use super::params::{EffectSampleScratch, PreparedEffectParams, RefValue};
use super::{FixtureContext, PixelContext, RuntimeError, RuntimeValue};

const MAX_LOOP_ITERATIONS: usize = 4096;
const INITIAL_RNG: u64 = 0x9e37_79b9_7f4a_7c15;

pub(super) fn run(
    program: &BytecodeProgram,
    progress: f64,
    seconds: f64,
    fixture: FixtureContext,
    pixel: PixelContext,
    params: &PreparedEffectParams,
) -> Result<Color, RuntimeError> {
    let mut scratch = EffectSampleScratch::new(stats_for_program(program, params.values.len()));
    run_with_scratch(
        program,
        progress,
        seconds,
        fixture,
        pixel,
        params,
        &mut scratch,
    )
}

pub(super) fn run_with_scratch<'a>(
    program: &'a BytecodeProgram,
    progress: f64,
    seconds: f64,
    fixture: FixtureContext,
    pixel: PixelContext,
    params: &'a PreparedEffectParams,
    scratch: &mut EffectSampleScratch,
) -> Result<Color, RuntimeError> {
    let program = params.specialized_bytecode.as_ref().unwrap_or(program);
    scratch.resize_for(program.registers);
    BytecodeVm {
        program,
        progress,
        seconds,
        fixture,
        pixel,
        params,
        scratch,
        ip: 0,
        rng: INITIAL_RNG,
        loop_iterations: 0,
    }
    .run()
}

struct BytecodeVm<'a, 'scratch> {
    program: &'a BytecodeProgram,
    progress: f64,
    seconds: f64,
    fixture: FixtureContext,
    pixel: PixelContext,
    params: &'a PreparedEffectParams,
    scratch: &'scratch mut EffectSampleScratch,
    ip: usize,
    rng: u64,
    loop_iterations: usize,
}

impl<'a> BytecodeVm<'a, '_> {
    fn run(&mut self) -> Result<Color, RuntimeError> {
        while let Some(instruction) = self.program.instructions.get(self.ip) {
            self.ip += 1;
            match *instruction {
                Instruction::LoadConst(dest, index) => {
                    if matches!(dest, ValueSlot::Ref(_, _)) {
                        self.write_ref_source(dest.reference(), RefValue::Constant(index));
                    } else {
                        self.write_runtime(dest, &self.program.constants[index])?;
                    }
                }
                Instruction::LoadContext(dest, slot) => self.write_context(dest, slot),
                Instruction::LoadParam(dest, index) => {
                    if matches!(dest, ValueSlot::Ref(_, _)) {
                        self.write_ref_source(dest.reference(), RefValue::Param(index));
                    } else {
                        self.write_runtime(dest, &self.params.values[index])?;
                    }
                }
                Instruction::Copy(dest, source) => self.copy(dest, source),
                Instruction::IntToFloat(dest, source) => {
                    self.scratch.floats[dest.0] = self.scratch.ints[source.0] as f64;
                }
                Instruction::FloatUnary(dest, UnaryFloatInstruction::Negate, source) => {
                    self.scratch.floats[dest.0] = -self.scratch.floats[source.0];
                }
                Instruction::IntNegate(dest, source) => {
                    self.scratch.ints[dest.0] = self.scratch.ints[source.0]
                        .checked_neg()
                        .ok_or_else(|| self.error("integer overflow"))?;
                }
                Instruction::BoolNot(dest, source) => {
                    self.scratch.bools[dest.0] = !self.scratch.bools[source.0];
                }
                Instruction::Binary(dest, op, left, right) => {
                    self.eval_binary(dest, op, left, right)?
                }
                Instruction::JumpIfFalse(condition, target) => {
                    if !self.scratch.bools[condition.0] {
                        self.ip = target;
                    }
                }
                Instruction::JumpIfTrue(condition, target) => {
                    if self.scratch.bools[condition.0] {
                        self.ip = target;
                    }
                }
                Instruction::Jump(target) => self.ip = target,
                Instruction::LoopTick => {
                    self.loop_iterations += 1;
                    if self.loop_iterations > MAX_LOOP_ITERATIONS {
                        return Err(self.error("effect exceeded the maximum loop iteration count"));
                    }
                }
                Instruction::Sin(dest, source) => {
                    self.scratch.floats[dest.0] = self.scratch.floats[source.0].sin();
                }
                Instruction::Cos(dest, source) => {
                    self.scratch.floats[dest.0] = self.scratch.floats[source.0].cos();
                }
                Instruction::Abs(dest, source) => {
                    self.scratch.floats[dest.0] = self.scratch.floats[source.0].abs();
                }
                Instruction::Floor(dest, source) => {
                    self.scratch.floats[dest.0] = self.scratch.floats[source.0].floor();
                }
                Instruction::Srand(dest, source) => {
                    self.rng = seed_from_float(self.scratch.floats[source.0]);
                    self.scratch.floats[dest.0] = 0.0;
                }
                Instruction::Rand(dest) => {
                    self.scratch.floats[dest.0] = self.rand();
                }
                Instruction::PixelIndex(dest, source) => {
                    self.scratch.ints[dest.0] = self.scratch.pixels[source.0].index as i64;
                }
                Instruction::PixelCount(dest, source) => {
                    self.scratch.ints[dest.0] = self.scratch.pixels[source.0].count as i64;
                }
                Instruction::PixelPosition(dest, source) => {
                    let pixel = self.scratch.pixels[source.0];
                    self.scratch.floats[dest.0] = pixel_position(pixel);
                }
                Instruction::SectionPosition(dest, source, width) => {
                    let pixel = self.scratch.pixels[source.0];
                    self.scratch.floats[dest.0] =
                        section_position(pixel, self.scratch.floats[width.0]);
                }
                Instruction::MarkCount(dest, source) => {
                    self.scratch.ints[dest.0] = self.marks(source)?.len() as i64;
                }
                Instruction::MarkAt(dest, marks, index, fallback) => {
                    let marks = self.marks(marks)?;
                    let fallback = self.scratch.floats[fallback.0];
                    let value = usize::try_from(self.scratch.ints[index.0])
                        .ok()
                        .and_then(|index| marks.get(index))
                        .copied()
                        .unwrap_or(fallback);
                    self.scratch.floats[dest.0] = value;
                }
                Instruction::MarkSearch(dest, search, marks, time, fallback) => {
                    let marks = self.marks(marks)?;
                    let time = self.scratch.floats[time.0];
                    let fallback = self.scratch.floats[fallback.0];
                    let value = match search {
                        MarkSearchInstruction::Prev => mark_prev(marks, time),
                        MarkSearchInstruction::Next => mark_next(marks, time),
                        MarkSearchInstruction::Nearest => mark_nearest(marks, time),
                        MarkSearchInstruction::Phase => mark_phase(marks, time),
                        MarkSearchInstruction::Elapsed => mark_elapsed(marks, time),
                    }
                    .unwrap_or(fallback);
                    self.scratch.floats[dest.0] = value;
                }
                Instruction::CurveCrossing(dest, curve, value, fallback) => {
                    let value = self.scratch.floats[value.0];
                    let fallback = self.scratch.floats[fallback.0];
                    self.scratch.floats[dest.0] =
                        self.curve_crossing(curve, value).unwrap_or(fallback);
                }
                Instruction::CurveParamCrossing(dest, index, value, fallback) => {
                    let value = self.scratch.floats[value.0];
                    let fallback = self.scratch.floats[fallback.0];
                    self.scratch.floats[dest.0] =
                        self.curve_param_crossing(index, value).unwrap_or(fallback);
                }
                Instruction::Min(dest, left, right) => {
                    self.scratch.floats[dest.0] =
                        self.scratch.floats[left.0].min(self.scratch.floats[right.0]);
                }
                Instruction::Max(dest, left, right) => {
                    self.scratch.floats[dest.0] =
                        self.scratch.floats[left.0].max(self.scratch.floats[right.0]);
                }
                Instruction::Clamp(dest, value, min, max) => {
                    self.scratch.floats[dest.0] = self.scratch.floats[value.0]
                        .clamp(self.scratch.floats[min.0], self.scratch.floats[max.0]);
                }
                Instruction::Smoothstep(dest, edge0, edge1, value) => {
                    let x = ((self.scratch.floats[value.0] - self.scratch.floats[edge0.0])
                        / (self.scratch.floats[edge1.0] - self.scratch.floats[edge0.0]))
                        .clamp(0.0, 1.0);
                    self.scratch.floats[dest.0] = x * x * (3.0 - 2.0 * x);
                }
                Instruction::MixFloat(dest, left, right, amount) => {
                    let left = self.scratch.floats[left.0];
                    let right = self.scratch.floats[right.0];
                    self.scratch.floats[dest.0] =
                        left + (right - left) * self.scratch.floats[amount.0];
                }
                Instruction::MixColor(dest, left, right, amount) => {
                    self.scratch.colors[dest.0] = self.scratch.colors[left.0]
                        .mix(self.scratch.colors[right.0], self.scratch.floats[amount.0]);
                }
                Instruction::Rgb(dest, red, green, blue) => {
                    self.scratch.colors[dest.0] = Color::new(
                        self.color_channel(red),
                        self.color_channel(green),
                        self.color_channel(blue),
                    );
                }
                Instruction::Hsv(dest, hue, saturation, value) => {
                    self.scratch.colors[dest.0] = hsv_to_rgb(
                        self.scratch.floats[hue.0],
                        self.scratch.floats[saturation.0],
                        self.scratch.floats[value.0],
                    );
                }
                Instruction::CallFloatCurveParam(dest, index, amount) => {
                    self.scratch.floats[dest.0] =
                        self.float_curve_param(index, self.scratch.floats[amount.0])?;
                }
                Instruction::CallColorCurveParam(dest, index, amount) => {
                    self.scratch.colors[dest.0] =
                        self.color_curve_param(index, self.scratch.floats[amount.0])?;
                }
                Instruction::CurveFloatClamped(dest, index, amount, min, max) => {
                    let value = self.float_curve_param(index, self.scratch.floats[amount.0])?;
                    self.scratch.floats[dest.0] =
                        value.clamp(self.scratch.floats[min.0], self.scratch.floats[max.0]);
                }
                Instruction::CurveColorScaled(dest, index, amount, level) => {
                    let level = self.scratch.floats[level.0];
                    self.scratch.colors[dest.0] = if level <= 0.0 {
                        Color::new(0, 0, 0)
                    } else {
                        self.color_curve_param(index, self.scratch.floats[amount.0])?
                            .scale(level)
                    };
                }
                Instruction::ReturnColor(source) => return Ok(self.scratch.colors[source.0]),
            }
        }
        Err(self.error("sample did not return"))
    }

    fn write_runtime(
        &mut self,
        dest: ValueSlot,
        value: &'a RuntimeValue,
    ) -> Result<(), RuntimeError> {
        match (dest, value) {
            (ValueSlot::Float(slot), RuntimeValue::Float(value)) => {
                self.scratch.floats[slot.0] = *value
            }
            (ValueSlot::Float(slot), RuntimeValue::Int(value)) => {
                self.scratch.floats[slot.0] = *value as f64
            }
            (ValueSlot::Int(slot), RuntimeValue::Int(value)) => self.scratch.ints[slot.0] = *value,
            (ValueSlot::Bool(slot), RuntimeValue::Bool(value)) => {
                self.scratch.bools[slot.0] = *value
            }
            (ValueSlot::Color(slot), RuntimeValue::Color(value)) => {
                self.scratch.colors[slot.0] = *value
            }
            (ValueSlot::Fixture(slot), RuntimeValue::Fixture(value)) => {
                self.scratch.fixtures[slot.0] = *value
            }
            (ValueSlot::Pixel(slot), RuntimeValue::Pixel(value)) => {
                self.scratch.pixels[slot.0] = *value
            }
            _ => return Err(self.error("bytecode value type mismatch")),
        }
        Ok(())
    }

    fn write_ref_source(&mut self, slot: RefSlot, value: RefValue) {
        self.scratch.refs[slot.0] = value;
    }

    fn write_context(&mut self, dest: ValueSlot, slot: ContextSlot) {
        match (dest, slot) {
            (ValueSlot::Float(dest), ContextSlot::Progress) => {
                self.scratch.floats[dest.0] = self.progress
            }
            (ValueSlot::Float(dest), ContextSlot::Seconds) => {
                self.scratch.floats[dest.0] = self.seconds
            }
            (ValueSlot::Fixture(dest), ContextSlot::Fixture) => {
                self.scratch.fixtures[dest.0] = self.fixture
            }
            (ValueSlot::Pixel(dest), ContextSlot::Pixel) => {
                self.scratch.pixels[dest.0] = self.pixel
            }
            _ => unreachable!("compiler emits matching context slots"),
        }
    }

    fn copy(&mut self, dest: ValueSlot, source: ValueSlot) {
        match (dest, source) {
            (ValueSlot::Float(dest), ValueSlot::Float(source)) => {
                self.scratch.floats[dest.0] = self.scratch.floats[source.0]
            }
            (ValueSlot::Int(dest), ValueSlot::Int(source)) => {
                self.scratch.ints[dest.0] = self.scratch.ints[source.0]
            }
            (ValueSlot::Bool(dest), ValueSlot::Bool(source)) => {
                self.scratch.bools[dest.0] = self.scratch.bools[source.0]
            }
            (ValueSlot::Color(dest), ValueSlot::Color(source)) => {
                self.scratch.colors[dest.0] = self.scratch.colors[source.0]
            }
            (ValueSlot::Ref(dest, _), ValueSlot::Ref(source, _)) => {
                self.scratch.refs[dest.0] = self.scratch.refs[source.0]
            }
            (ValueSlot::Fixture(dest), ValueSlot::Fixture(source)) => {
                self.scratch.fixtures[dest.0] = self.scratch.fixtures[source.0]
            }
            (ValueSlot::Pixel(dest), ValueSlot::Pixel(source)) => {
                self.scratch.pixels[dest.0] = self.scratch.pixels[source.0]
            }
            _ => unreachable!("compiler emits matching copy slots"),
        }
    }

    fn eval_binary(
        &mut self,
        dest: ValueSlot,
        op: BinaryInstruction,
        left: ValueSlot,
        right: ValueSlot,
    ) -> Result<(), RuntimeError> {
        match op {
            BinaryInstruction::FloatAdd => {
                self.write_float_dest(dest, self.float(left) + self.float(right))
            }
            BinaryInstruction::FloatSubtract => {
                self.write_float_dest(dest, self.float(left) - self.float(right))
            }
            BinaryInstruction::FloatMultiply => {
                self.write_float_dest(dest, self.float(left) * self.float(right))
            }
            BinaryInstruction::FloatDivide => {
                self.write_float_dest(dest, self.float(left) / self.float(right))
            }
            BinaryInstruction::IntAdd => self.write_int_dest(
                dest,
                self.int(left)
                    .checked_add(self.int(right))
                    .ok_or_else(|| self.error("integer overflow"))?,
            ),
            BinaryInstruction::IntSubtract => self.write_int_dest(
                dest,
                self.int(left)
                    .checked_sub(self.int(right))
                    .ok_or_else(|| self.error("integer overflow"))?,
            ),
            BinaryInstruction::IntMultiply => self.write_int_dest(
                dest,
                self.int(left)
                    .checked_mul(self.int(right))
                    .ok_or_else(|| self.error("integer overflow"))?,
            ),
            BinaryInstruction::IntDivide => {
                let right = self.int(right);
                if right == 0 {
                    return Err(self.error("integer divide by zero"));
                }
                self.write_int_dest(
                    dest,
                    self.int(left)
                        .checked_div(right)
                        .ok_or_else(|| self.error("integer overflow"))?,
                );
            }
            BinaryInstruction::FloatLess => {
                self.write_bool_dest(dest, self.float(left) < self.float(right))
            }
            BinaryInstruction::FloatLessEqual => {
                self.write_bool_dest(dest, self.float(left) <= self.float(right))
            }
            BinaryInstruction::FloatGreater => {
                self.write_bool_dest(dest, self.float(left) > self.float(right))
            }
            BinaryInstruction::FloatGreaterEqual => {
                self.write_bool_dest(dest, self.float(left) >= self.float(right))
            }
            BinaryInstruction::IntLess => {
                self.write_bool_dest(dest, self.int(left) < self.int(right))
            }
            BinaryInstruction::IntLessEqual => {
                self.write_bool_dest(dest, self.int(left) <= self.int(right))
            }
            BinaryInstruction::IntGreater => {
                self.write_bool_dest(dest, self.int(left) > self.int(right))
            }
            BinaryInstruction::IntGreaterEqual => {
                self.write_bool_dest(dest, self.int(left) >= self.int(right))
            }
            BinaryInstruction::FloatEqual => {
                self.write_bool_dest(dest, self.float(left) == self.float(right))
            }
            BinaryInstruction::FloatNotEqual => {
                self.write_bool_dest(dest, self.float(left) != self.float(right))
            }
            BinaryInstruction::IntEqual => {
                self.write_bool_dest(dest, self.int(left) == self.int(right))
            }
            BinaryInstruction::IntNotEqual => {
                self.write_bool_dest(dest, self.int(left) != self.int(right))
            }
            BinaryInstruction::BoolEqual => {
                self.write_bool_dest(dest, self.bool(left) == self.bool(right))
            }
            BinaryInstruction::BoolNotEqual => {
                self.write_bool_dest(dest, self.bool(left) != self.bool(right))
            }
            BinaryInstruction::EnumEqual => {
                self.write_bool_dest(dest, self.enum_value(left)? == self.enum_value(right)?)
            }
            BinaryInstruction::EnumNotEqual => {
                self.write_bool_dest(dest, self.enum_value(left)? != self.enum_value(right)?)
            }
            BinaryInstruction::ColorMultiplyFloat => {
                self.write_color_dest(dest, self.color(left).scale(self.float(right)));
            }
            BinaryInstruction::FloatMultiplyColor => {
                self.write_color_dest(dest, self.color(right).scale(self.float(left)));
            }
        }
        Ok(())
    }

    fn write_float_dest(&mut self, dest: ValueSlot, value: f64) {
        self.scratch.floats[dest.float().0] = value;
    }

    fn write_int_dest(&mut self, dest: ValueSlot, value: i64) {
        self.scratch.ints[dest.int().0] = value;
    }

    fn write_bool_dest(&mut self, dest: ValueSlot, value: bool) {
        self.scratch.bools[dest.bool().0] = value;
    }

    fn write_color_dest(&mut self, dest: ValueSlot, value: Color) {
        self.scratch.colors[dest.color().0] = value;
    }

    fn float(&self, slot: ValueSlot) -> f64 {
        self.scratch.floats[slot.float().0]
    }

    fn int(&self, slot: ValueSlot) -> i64 {
        self.scratch.ints[slot.int().0]
    }

    fn bool(&self, slot: ValueSlot) -> bool {
        self.scratch.bools[slot.bool().0]
    }

    fn color(&self, slot: ValueSlot) -> Color {
        self.scratch.colors[slot.color().0]
    }

    fn marks(&self, slot: RefSlot) -> Result<&[f64], RuntimeError> {
        let value = match self.scratch.refs[slot.0] {
            RefValue::Param(index) => &self.params.values[index],
            RefValue::Constant(index) => &self.program.constants[index],
            RefValue::Unset => return Err(self.error("expected marks value")),
        };
        match value {
            RuntimeValue::Marks(value) => Ok(value),
            _ => Err(self.error("expected marks value")),
        }
    }

    fn curve(&self, slot: RefSlot) -> Result<&Curve, RuntimeError> {
        let value = match self.scratch.refs[slot.0] {
            RefValue::Param(index) => &self.params.values[index],
            RefValue::Constant(index) => &self.program.constants[index],
            RefValue::Unset => return Err(self.error("expected curve value")),
        };
        match value {
            RuntimeValue::Curve(value) => Ok(value),
            _ => Err(self.error("expected curve value")),
        }
    }

    fn curve_crossing(&self, slot: RefSlot, value: f64) -> Option<f64> {
        match self.scratch.refs[slot.0] {
            RefValue::Param(index) => self.curve_param_crossing(index, value),
            RefValue::Constant(_) | RefValue::Unset => self
                .curve(slot)
                .ok()
                .and_then(|curve| curve_crossing(curve, value)),
        }
    }

    fn curve_param_crossing(&self, index: usize, value: f64) -> Option<f64> {
        self.params
            .curve_crossings
            .get(index)
            .and_then(Option::as_ref)
            .and_then(|table| table.crossing(value))
    }

    fn float_curve_param(&self, index: usize, amount: f64) -> Result<f64, RuntimeError> {
        let RuntimeValue::Curve(curve) = &self.params.values[index] else {
            return Err(self.error("expected curve parameter"));
        };
        match curve.evaluate(amount) {
            Some(CurveValue::Float(value)) => Ok(value),
            Some(CurveValue::Color(_)) => Err(self.error("expected float curve parameter")),
            None => Err(self.error("curve has no points")),
        }
    }

    fn color_curve_param(&self, index: usize, amount: f64) -> Result<Color, RuntimeError> {
        let RuntimeValue::Curve(curve) = &self.params.values[index] else {
            return Err(self.error("expected curve parameter"));
        };
        match curve.evaluate(amount) {
            Some(CurveValue::Color(value)) => Ok(value),
            Some(CurveValue::Float(_)) => Err(self.error("expected color curve parameter")),
            None => Err(self.error("curve has no points")),
        }
    }

    fn enum_value(&self, slot: ValueSlot) -> Result<&str, RuntimeError> {
        let slot = slot.reference();
        let value = match self.scratch.refs[slot.0] {
            RefValue::Param(index) => &self.params.values[index],
            RefValue::Constant(index) => &self.program.constants[index],
            RefValue::Unset => return Err(self.error("expected enum value")),
        };
        match value {
            RuntimeValue::Enum(value) => Ok(value),
            _ => Err(self.error("expected enum value")),
        }
    }

    fn color_channel(&self, slot: FloatSlot) -> u8 {
        self.scratch.floats[slot.0].round().clamp(0.0, 255.0) as u8
    }

    fn error(&self, message: &str) -> RuntimeError {
        RuntimeError {
            message: message.to_string(),
        }
    }

    fn rand(&mut self) -> f64 {
        self.rng = self
            .rng
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.rng >> 11) as f64) / ((1u64 << 53) as f64)
    }
}

fn seed_from_float(value: f64) -> u64 {
    let mut seed = value.to_bits();
    seed ^= seed >> 30;
    seed = seed.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    seed ^= seed >> 27;
    seed = seed.wrapping_mul(0x94d0_49bb_1331_11eb);
    seed ^ (seed >> 31)
}

fn pixel_position(pixel: PixelContext) -> f64 {
    if pixel.count <= 1 {
        0.0
    } else {
        pixel.index as f64 / (pixel.count - 1) as f64
    }
}

fn section_position(pixel: PixelContext, width: f64) -> f64 {
    let width = width.floor().max(1.0);
    let section_count = ((pixel.count as f64 + width - 1.0) / width)
        .floor()
        .max(1.0);
    let section_index = (pixel.index as f64 / width).floor();
    if section_count <= 1.0 {
        0.0
    } else {
        section_index / (section_count - 1.0)
    }
}

fn hsv_to_rgb(hue: f64, saturation: f64, value: f64) -> Color {
    let hue = hue.rem_euclid(360.0) / 60.0;
    let c = value.clamp(0.0, 1.0) * saturation.clamp(0.0, 1.0);
    let x = c * (1.0 - ((hue % 2.0) - 1.0).abs());
    let m = value.clamp(0.0, 1.0) - c;
    let (red, green, blue) = if hue < 1.0 {
        (c, x, 0.0)
    } else if hue < 2.0 {
        (x, c, 0.0)
    } else if hue < 3.0 {
        (0.0, c, x)
    } else if hue < 4.0 {
        (0.0, x, c)
    } else if hue < 5.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    Color::new(
        ((red + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((green + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((blue + m) * 255.0).round().clamp(0.0, 255.0) as u8,
    )
}

fn mark_prev(marks: &[f64], time: f64) -> Option<f64> {
    let index = marks.partition_point(|mark| *mark <= time);
    index.checked_sub(1).map(|index| marks[index])
}

fn mark_next(marks: &[f64], time: f64) -> Option<f64> {
    marks
        .get(marks.partition_point(|mark| *mark <= time))
        .copied()
}

fn mark_nearest(marks: &[f64], time: f64) -> Option<f64> {
    let previous = mark_prev(marks, time);
    let next = mark_next(marks, time);
    match (previous, next) {
        (Some(previous), Some(next)) if (time - previous) <= (next - time) => Some(previous),
        (Some(_), Some(next)) => Some(next),
        (Some(previous), None) => Some(previous),
        (None, Some(next)) => Some(next),
        (None, None) => None,
    }
}

fn mark_phase(marks: &[f64], time: f64) -> Option<f64> {
    let previous = mark_prev(marks, time)?;
    if (time - previous).abs() < f64::EPSILON {
        return Some(0.0);
    }
    let next = mark_next(marks, time)?;
    let span = next - previous;
    if span <= f64::EPSILON {
        return None;
    }
    Some(((time - previous) / span).clamp(0.0, 1.0))
}

fn mark_elapsed(marks: &[f64], time: f64) -> Option<f64> {
    mark_prev(marks, time).map(|previous| time - previous)
}

fn curve_crossing(curve: &Curve, value: f64) -> Option<f64> {
    let mut previous = curve.points.first()?;
    for point in &curve.points[1..] {
        let CurveValue::Float(left) = previous.value else {
            return None;
        };
        let CurveValue::Float(right) = point.value else {
            return None;
        };
        if (left <= value && right >= value) || (left >= value && right <= value) {
            let span = right - left;
            let amount = if span.abs() < f64::EPSILON {
                0.0
            } else {
                (value - left) / span
            };
            return Some(previous.time + (point.time - previous.time) * amount);
        }
        previous = point;
    }
    None
}
