# Dawn v1 Language Specification

This document describes the normative Dawn v1 language with examples and
semantic rules. The complete grammar lives in
[dawn-language-grammar.md](dawn-language-grammar.md).

Dawn v1 is a family of typed documents with shared lexical rules, top-level
typed imports, typed values, diagnostics, and one project-owned symbol graph.
Project, display, hardware, layout, patch, sequence, and curve documents are
declarative. Effect documents are first-class `effect Name { ... }` documents
with typed parameters and expression bodies.

## Source Model

Every Dawn source file contains zero or more top-level imports followed by
exactly one document. Imports must appear before the document and must not
appear inside document or declaration bodies.

Imports bind named typed documents, not textual fragments. The imported
document's actual kind must match the kind named in the import. Import paths are
resolved relative to the importing file.

```dawn
import display Main from <displays/Main.display.dawn>;
import sequence Demo from <sequences/Demo.sequence.dawn>;

project DemoProject {
  version 1;

  display Main;
  sequence Demo;
}
```

Comments are line comments:

```dawn
// Imports stay at the top of the file.
import effect Pulse from <../effects/Pulse.effect.dawn>;
```

Semicolons are required after declarations and statements except block endings.

## Lexical Values

Identifiers are ASCII and case-sensitive. Document names use `PascalCase`.
Parameters, enums, flags, and variants use `lower_snake`.

String literals are valid only for user-facing `label` and `description`
fields. They are not effect parameter values, protocol identifiers, vendors,
models, output types, color orders, serial protocols, filter types, behavioral
fields, or arbitrary metadata. All non-display values remain typed identifiers,
ints, floats, bools, addresses, paths, durations, or colors.

```dawn
// Valid values.
duration 1m20s40ms;
audio <audio/demo.wav>;
address 192.168.1.50;
base #40c4ff;
label "Front left roofline";
```

Duration literals are additive chains of `m`, `s`, and `ms` parts. The `ms`
suffix is matched before `m`.

```dawn
duration 500ms;
duration 10s40ms;
duration 1m20s;
duration 1m2.5s40ms;
```

Path literals are angle-bracket literals and are valid only in import and
resource positions such as `from <...>`, `audio <...>;`, and serial
`device <...>;`.

Controller addresses use IPv4 address literals. Implementations must reject
octets outside `0..255`.

## Complete Project Example

`Project.dawn`:

```dawn
import display Main from <displays/Main.display.dawn>;
import sequence Demo from <sequences/Demo.sequence.dawn>;

project DemoProject {
  version 1;

  display Main;
  sequence Demo;
}
```

`displays/Main.display.dawn`:

```dawn
import hardware StageHardware from <../hardware/StageHardware.hardware.dawn>;
import layout StageLayout from <../layouts/StageLayout.layout.dawn>;
import patch StagePatch from <../patches/StagePatch.patch.dawn>;

display Main {
  version 1;

  hardware StageHardware;
  layout StageLayout;
  patch StagePatch;
}
```

`hardware/StageHardware.hardware.dawn`:

```dawn
hardware StageHardware {
  version 1;

  controller StagePixels {
    vendor falcon;
    model F16V5;

    input MainE131 {
      type e131;
      port 5568;
      priority 100;

      universe U001 {
        id 1;
        start_channel 1;
        channels 450;
        address 192.168.1.50;
        cast multicast;
        enabled true;
      }

      universe U002 {
        id 2;
        start_channel 451;
        channels 300;
        address 192.168.1.50;
        cast multicast;
      }
    }

    output Local1 {
      type pixel_string;
      input MainE131;
      input_channel 1;
      connector local;
      index 1;
      pixel_protocol ws2811;
      node_count 150;
      color_order grb;
      white_mode none;
      null_nodes 0;
      end_null_nodes 0;
      group_size 1;
      reverse false;
      zigzag 0;
      brightness 0.9;
      gamma 2.2;
    }
  }

  controller WledAccent {
    vendor wled;
    model esp32;

    input DdpIn {
      type ddp;
      address 192.168.1.60;
      port 4048;
      destination 1;
      channel_count 6315;
      channels_per_packet 1440;
      keep_channel_numbers false;
      byte_offset 30;
    }

    output Strip0 {
      type pixel_string;
      input DdpIn;
      input_channel 31;
      connector gpio;
      index 16;
      pixel_protocol ws2812b;
      node_count 200;
      color_order grb;
      white_mode none;
      null_nodes 10;
      end_null_nodes 0;
      group_size 1;
      reverse true;
      zigzag 0;
      brightness 0.9;
      gamma 2.2;
    }
  }
}
```

`patches/StagePatch.patch.dawn`:

```dawn
patch StagePatch {
  version 1;

  route LeftStripToPort {
    from layout.LeftStrip;
    to hardware.StagePixels.Local1;
  }
}
```

## Controller Hardware

Controller modeling has three explicit layers:

- `controller`: identity and transport owner, with `vendor`, `model`, optional
  `variant`, and one or more inputs and outputs.
- `input`: received data stream. Every input exposes one logical channel space
  that outputs and patch routes can target.
- `output`: physical or logical channel endpoint bound to an input channel
  range, with connector identity, derived or declared capacity, and driver
  transforms.

Supported input types are:

- `e131`: `port`, optional `priority`, optional `sync_universe`, and one or
  more explicit `universe` rows.
- `artnet`: `port` and one or more explicit `universe` rows. Art-Net universe
  rows may carry `net` and `subnet` when needed.
- `ddp`: `address`, `port`, `destination`, `channel_count`, optional
  `channels_per_packet`, optional `keep_channel_numbers`, and optional
  `byte_offset`.
- `serial`: `device`, `baud`, `data_bits`, `parity`, `stop_bits`, `protocol`,
  and `channel_count`. Serial protocol identifiers include `dmx`, `opendmx`,
  `renard`, `lor`, `dlight`, `pixelnet`, and `generic_serial`.

E1.31 and Art-Net use explicit universe rows:

```dawn
input MainE131 {
  type e131;
  port 5568;
  priority 100;

  universe U100 {
    id 100;
    start_channel 1;
    channels 15;
    address 192.168.0.50;
    cast unicast;
    enabled true;
  }

  universe U101 {
    id 101;
    start_channel 16;
    channels 426;
    address 192.168.0.50;
    cast unicast;
  }
}
```

Each active row declares an input logical span
`start_channel..start_channel + channels - 1`. E1.31 and Art-Net rows must
declare `1..512` channels. Disabled rows remain source-representable for
round-tripping but do not contribute active capacity or duplicate destination
checks.

DDP and serial inputs are streams, not universe lists:

```dawn
input DdpIn {
  type ddp;
  address 192.168.1.60;
  port 4048;
  destination 1;
  channel_count 6315;
  channels_per_packet 1440;
  keep_channel_numbers false;
  byte_offset 30;
}

input RenardBus {
  type serial;
  device <COM3>;
  baud 57600;
  data_bits 8;
  parity none;
  stop_bits 1;
  protocol renard;
  channel_count 48;
}
```

Outputs bind physical endpoints to the input channel space explicitly. Pixel
string capacity is derived from `node_count * arity(color_order)`. RGB orders
such as `rgb`, `grb`, and `brg` have arity 3; RGBW orders such as `rgbw`,
`grbw`, and `brgw` have arity 4. `white_mode` describes behavior and does not
change channel arity.

```dawn
output Local1 {
  type pixel_string;
  input MainE131;
  input_channel 1;
  connector local;
  index 1;
  pixel_protocol ws2811;
  node_count 150;
  color_order grb;
  white_mode none;
  null_nodes 0;
  end_null_nodes 0;
  group_size 1;
  reverse false;
  zigzag 0;
  brightness 0.9;
  gamma 2.2;
}
```

Supported output types are:

- `pixel_string`: addressable pixels or nodes. Fields include
  `pixel_protocol`, `node_count`, `color_order`, `white_mode`, `null_nodes`,
  `end_null_nodes`, `group_size`, `reverse`, `zigzag`, `brightness`, `gamma`,
  optional `color_order_override` spans, and optional `virtual_string` blocks.
  Its capacity is derived from `node_count * arity(color_order)`.
- `dmx`: either raw DMX-style channels with explicit `channel_count`, or a
  typed fixture bank with `fixture_count` and `color_order`. Typed fixture-bank
  capacity is `fixture_count * arity(color_order)`. `fixture_profile` may be
  supplied as metadata by identifier.
- `dumb_rgb` and `dumb_rgbw`: fixed multi-channel strings or floods. Fields
  include `fixture_count` and `color_order`; capacity is derived from color
  arity.
- `single_channel`: AC relay, dimmer, strobe, or other one-channel endpoints.
  Fields include `channel_count`.

Virtual strings model xLights, Falcon, Kulp, smart receiver, and chained model
spans under a physical output. They are targetable as
`Hardware.Controller.Output.VirtualString`. Virtual strings inherit the parent
pixel string's behavior. They require only `input_channel` and `node_count`;
`color_order`, `white_mode`, `null_nodes`, `end_null_nodes`, `group_size`,
`reverse`, `zigzag`, `brightness`, and `gamma` may be overridden only when the
virtual string differs from its parent. Virtual-string capacity is derived from
`node_count * arity(effective color_order)`.

```dawn
output ReceiverA1 {
  type pixel_string;
  input MainE131;
  input_channel 751;
  connector receiver;
  index 1;
  pixel_protocol ws2811;
  node_count 150;
  color_order rgb;
  white_mode none;
  null_nodes 2;
  end_null_nodes 3;
  group_size 3;
  reverse false;
  zigzag 20;
  brightness 0.8;
  gamma 2.0;

  virtual_string ReceiverSpanA {
    input_channel 751;
    node_count 100;
    color_order brg;
    end_null_nodes 0;
    group_size 2;
    brightness 0.7;
  }
}
```

## Patch Graphs

Patch documents define graph nodes and routed channel spans. A patch document
does not import the concrete hardware or layout it references. Instead, the
owning `display` is the composition root: it imports and selects one hardware
document, one layout document, and one patch document. Patch endpoint resolution
uses that display context.

Routes may omit `from_channel`, `to_channel`, and `channels` for an unambiguous
full-span mapping. If any span field is present, all three must be present.
Layout fixture and group routes to color-aware outputs infer RGB/RGBW expansion
from the destination `color_order`. Filters remain available for nontrivial
graph transforms, but filter declarations do not declare author-written input
or output sizes; their capacity is derived from connected endpoints or filter
type semantics.

Endpoints may be:

- Layout fixtures or groups through the contextual root, such as
  `layout.LeftStrip`.
- Filter nodes declared in the same patch document, such as `FrontRgb`.
- Hardware inputs through the contextual root, such as
  `hardware.WledAccent.DdpIn`.
- Input universe rows, such as `hardware.DmxNode.ArtNetIn.U010`.
- Physical outputs, such as `hardware.StagePixels.Local1`.
- Virtual strings, such as `hardware.StagePixels.Local1.FrontLeftModel`.

```dawn
patch StagePatch {
  version 1;

  route LeftStripToPort {
    from layout.LeftStrip;
    to hardware.StagePixels.Local1;
  }

  route LeftStripToVirtualString {
    from layout.LeftStrip;
    from_channel 1;
    to hardware.StagePixels.Local1.FrontLeftModel;
    to_channel 1;
    channels 75;
  }
}
```

Importers may emit explicit one-channel routes for arbitrary Vixen patch lines
and may coalesce contiguous equivalent routes when the graph remains equivalent.

## Layouts

Layout documents model spatial fixtures and groups. A fixture's `pixel_count`
declares its logical source element capacity before destination color expansion.
A group is a named set of fixtures or other groups.

```dawn
layout StageLayout {
  version 1;

  fixture LeftStrip {
    shape line(start (0.0, 0.0), end (0.0, 1.0));
    pixel_count 150;
  }

  group Front {
    members [LeftStrip];
  }
}
```

## Curves

A curve is a reusable mapping from a normalized numeric input to a typed output
value:

```text
curve<T> = number in 0.0..1.0 -> T
```

Float automation curve:

```dawn
curve PulseEnvelope {
  version 1;
  range float(0.0, 1.0);

  key 0.0 0.0 linear;
  key 0.25 1.0 ease_out;
  key 1.0 0.4 ease_in_out;
}
```

Color ramps are curves with `range color`.

```dawn
curve SunsetPalette {
  version 1;
  range color;

  key 0.0 #1b1b5f linear;
  key 0.5 #ff4fd8 ease_in_out;
  key 1.0 #ffd166 linear;
}
```

Curve keys must be ordered by increasing input value and must stay within
`0.0..1.0`.

## Effects

Effects are the only Dawn v1 documents with a full expression language. An
effect body must return `color`.

Effect parameters may be `float(min,max)`, `int(min,max)`, `bool`, `color`,
`curve`, `curve<T>`, or named `enum`/`flags` types. Effect parameters cannot be
strings, text, or paths in Dawn v1.

```dawn
effect Pulse {
  version 1;
  spatial true;

  enum speed_mode { slow, med, fast }
  flags channels { red, green, blue }

  param speed: float(0.1, 10.0) = 1.0;
  param mode: speed_mode = speed_mode.med;
  param enabled: channels = channels.red | channels.green | channels.blue;
  param base: color = #40c4ff;
  param intensity: curve<float(0.0, 1.0)>;
  param palette: curve<color>;

  let rate = switch mode {
    case speed_mode.slow: 0.5;
    case speed_mode.med: 1.0;
    case speed_mode.fast: 2.0;
    default: 1.0;
  };

  let phase = fract(t * speed * rate);
  let wave = 0.35 + 0.65 * abs(sin((t + pos) * speed * TAU));
  let colorized = mix(base, palette(pos), 0.5);

  if wave > 0.1 {
    colorized * wave * intensity(phase)
  } else {
    #000
  }
}
```

Local enum and flag defaults use `TypeName.variant`. Sequences use
cross-document qualification such as `Pulse.speed_mode.fast`.

## Sequences

Sequences import effects and reusable parameter documents such as curves, then
invoke effects with typed named parameters.

```dawn
sequence Demo {
  version 1;

  duration 1m30s;
  frame_rate 60;
  audio <audio/demo.wav>;

  event Intro {
    at 0s for 10s;
    effect Pulse(
      speed 0.8,
      mode Pulse.speed_mode.slow,
      enabled Pulse.channels.blue,
      base #40c4ff,
      intensity PulseEnvelope,
      palette SunsetPalette
    );
  }
}
```

At sequence compile time, implementations check that every named argument
exists on the target effect and that each argument has the expected type and
range.

## Types

Primitive source types are:

`int`, `float`, `bool`, `color`, `duration`, `address`, `path`

Display-only source fields:

`label`, `description`

Effect parameter types are:

`float(min,max)`, `int(min,max)`, `bool`, `color`, `curve`, `curve<T>`, named
`enum`, and named `flags`

`path` remains a source value type for imports and resources, but not an effect
parameter type. `string_lit` remains display-only and is not a general value
expression.

## Validation

Implementations must reject:

- Missing import targets.
- Import cycles.
- Imported document kind mismatches.
- References to unknown documents or declarations.
- Display documents that use a patch without selecting exactly one hardware
  document and exactly one layout document.
- Patch route endpoints using `hardware.*` or `layout.*` outside a display
  context.
- Patch route endpoints whose contextual `hardware` or `layout` root cannot be
  resolved through the owning display.
- E1.31 or Art-Net universe rows outside `1..512` channels.
- Overlapping active universe logical spans in the same input.
- Duplicate active universe destination tuples in the same input. For E1.31,
  the tuple is protocol, address, port, universe id, and cast. For Art-Net, the
  tuple is protocol, address, port, net, subnet, universe id, and cast.
- DDP inputs with universe rows.
- Output input spans that do not fit inside the referenced input's declared or
  derived channel space.
- Output input spans that do not resolve through declared E1.31 or Art-Net
  segments.
- Physical output or virtual-string target spans outside their declared or
  derived capacity.
- Explicit redundant capacity fields on pixel strings or virtual strings,
  including `channel_count` and `channels_per_node`.
- `color_order` values whose arity cannot be derived as RGB or RGBW.
- Unknown patch route endpoints.
- Route channel spans outside declared source or destination capacities.
- Routes that provide only one or two of `from_channel`, `to_channel`, and
  `channels`.
- Routes with omitted spans where source or destination capacity cannot be
  inferred unambiguously.
- Routes whose inferred color expansion is impossible for the destination
  shape.
- Serial outputs on hardware without serial transport.
- Ethernet-only inputs without required network address and port fields.

## Diagnostics

Dawn implementations must report diagnostics with a source span when the error
is tied to source text. Diagnostics should include the expected document kind,
type, parameter name, route endpoint, or variant name where that information is
available.

Required diagnostics include:

- Unknown keyword, declaration, field, document kind, or symbol.
- Invalid lexical token.
- Missing required semicolon.
- Import declaration after the document begins or inside another scope.
- Invalid path literal position.
- String literal outside `label` or `description`.
- Invalid address literal.
- Import path resolution failure.
- Imported document kind mismatch.
- Import cycle.
- Overlapping universe rows.
- Duplicate active universe destinations.
- Missing display hardware or layout context for a patch.
- Ambiguous display hardware or layout context for a patch.
- Contextual patch endpoint outside a display context.
- Contextual patch endpoint root that is not `hardware`, `layout`, or a local
  filter node.
- Output input span outside referenced input capacity.
- Output input span not covered by declared universe rows.
- Unknown controller input, output, or virtual string.
- Unresolved route endpoint.
- Route span overflow.
- Derived capacity mismatch or invalid explicit redundant capacity.
- Invalid `color_order` arity.
- Ambiguous omitted route span.
- Partial route span missing required fields.
- Impossible inferred color expansion.
- Serial output without serial transport.
- Unknown effect parameter.
- Missing required effect parameter.
- Parameter type or range mismatch.
- Invalid enum or flag qualification.
- Curve key input outside `0.0..1.0`.
- Invalid curve key range value.
- Unordered curve keys.
- Curve argument range mismatch.
- Invalid curve sample input.
- Non-`color` effect body.

## Invalid Examples

Imports inside documents are invalid:

```dawn
display Main {
  version 1;
  import hardware StageHardware from <../hardware/StageHardware.hardware.dawn>;
}
```

Strings are invalid outside user-facing display fields:

```dawn
effect Bad {
  version 1;
  param label: text = "fast";
  #fff
}
```

DDP does not use universes:

```dawn
input BadDdp {
  type ddp;
  address 192.168.1.60;
  port 4048;
  destination 1;
  channel_count 510;

  universe U001 {
    id 1;
    start_channel 1;
    channels 510;
    address 192.168.1.60;
    cast unicast;
  }
}
```

Pixel strings cannot restate derived capacity:

```dawn
output BadPixels {
  type pixel_string;
  input MainE131;
  input_channel 1;
  channel_count 450;
  connector local;
  index 1;
  pixel_protocol ws2811;
  node_count 150;
  color_order grb;
  white_mode none;
  null_nodes 0;
  end_null_nodes 0;
  group_size 1;
  reverse false;
  zigzag 0;
  brightness 0.9;
  gamma 2.2;
}
```

Virtual strings cannot restate parent capacity fields:

```dawn
virtual_string BadSpan {
  input_channel 1;
  node_count 75;
  channels_per_node 3;
}
```

Partial route spans must provide all three span fields:

```dawn
route BadPartial {
  from layout.LeftStrip;
  from_channel 1;
  to hardware.StagePixels.Local1;
}
```

Omitted route spans are invalid when capacity cannot be inferred
unambiguously:

```dawn
route BadInference {
  from layout.DmxWash;
  to hardware.DmxNode.ArtNetIn.U010;
}
```

Inferred color expansion must fit the destination shape:

```dawn
route BadExpansion {
  from layout.LeftStrip;
  to hardware.DmxNode.DmxOut1;
}
```

Color order must have a known RGB or RGBW arity:

```dawn
output BadOrder {
  type pixel_string;
  input MainE131;
  input_channel 1;
  connector local;
  index 1;
  pixel_protocol ws2811;
  node_count 150;
  color_order rg;
  white_mode none;
  null_nodes 0;
  end_null_nodes 0;
  group_size 1;
  reverse false;
  zigzag 0;
  brightness 0.9;
  gamma 2.2;
}
```

Enum variants must be qualified:

```dawn
event Drop {
  at 10s for 2s;
  effect Pulse(mode fast);
}
```

Path literals cannot be effect parameter values:

```dawn
effect BadPath {
  version 1;
  param image: path = <images/pattern.png>;
  #fff
}
```

## Parser Tooling

Dawn v1 should remain spec-first while syntax settles. Implementations may keep
handwritten parsers during this phase so diagnostics and language experiments
remain straightforward.

Future parser tooling options include:

- [LALRPOP](https://github.com/lalrpop/lalrpop) for a Rust LR(1) compiler
  parser.
- [Logos](https://logos.maciej.codes/) as a generated lexer if replacing
  handwritten lexers.
- [pest](https://pest.rs/book/grammars/grammars) for PEG grammar files.
- [Tree-sitter](https://tree-sitter.github.io/tree-sitter/creating-parsers/1-getting-started.html)
  for editor and incremental parsing.
- [chumsky](https://docs.rs/chumsky/latest/chumsky/guide/),
  [winnow](https://docs.rs/winnow/latest/winnow/), and
  [nom](https://docs.rs/nom/latest/nom/) as parser-combinator options rather
  than normative grammar files.
