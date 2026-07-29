# Sequence-as-Code Contract

A Dawn sequence is declarative YAML that names a duration, frame rate, layers,
effect instances, a composition graph, automation clips, and control clips. It
is loaded into the typed `dawn_language::sequence::Sequence`; YAML is never an
editable runtime model after load.

## Validity

The canonical validator is `dawn_language::validation::validate_sequence`.
Project loading and checking, accepted GUI edits, and runtime preparation all
use that validator. A sequence must satisfy these rules:

- `duration` is finite, non-negative source input and is positive once loaded.
- `frame_rate` is greater than zero, and `duration * frame_rate` cannot exceed
  250,000 prepared frames.
- Layer, effect, mark-collection, automation-clip, and control-clip IDs are
  unique. Timed objects fit within the sequence duration.
- Effects reference an existing layer, a compatible color target, a defined
  effect, and only its declared parameters. Required parameters must be
  supplied; unknown parameters are invalid.
- The composition graph has one output and valid acyclic, typed connections.
  Its layer nodes reference distinct sequence layers.
- Active automation targets exist, have a compatible mapping, and are unique
  even relative to detached bindings. Detached bindings preserve a historical
  unresolved target only after an explicit detachment reason.
- Control clips are valid, in range, and non-overlapping for the same target.

## Curves

Curves are normalized, piecewise-linear values. They must contain at least one
point; each point’s position and value must be finite; positions are in
`[0, 1]` and strictly increasing. All sequence automation, native effects, and
DSL curve reads use `dawn_language::sampling::sample_curve`.

## Source diagnostics

Present optional values must have their declared shape. For example,
`automation_clips: wrong` is an error, not an empty clip list. Sequence fields
are closed: unknown keys are errors, so misspellings cannot be lost during a
typed save. Duration parsing is fallible and never invokes panicking duration
constructors.

The effect and operator DSL reports parse/type errors rather than changing a
bad literal to zero. Integer division produces a float; integer overflow and
remainder by zero are VM errors. Required DSL parameters cannot receive an
implicit type default.

## Runtime budgets

The renderer limits a prepared sequence to 250,000 frames, generated effects
to 100,000 per preparation, and custom-operator Signal sampling to 4,096 unique
times per operator render. The DSL VM limits each invocation to 10,000 loop
iterations. Exceeding a budget returns an error; Dawn does not silently clamp
or skip work.

## Authoring

- Use `.sequence.dawn` YAML for sequence data.
- Use `.effect.dawn` for custom effects and `.operator.dawn` for custom graph
  operators; both receive DSL highlighting in the desktop editor.
- Start from `examples/starter` for valid curve, effect, operator, and graph
  examples.
