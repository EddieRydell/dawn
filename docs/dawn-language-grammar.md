# Dawn v1 EBNF Grammar

This document contains the normative Dawn v1 grammar. See
[dawn-language-spec.md](dawn-language-spec.md) for examples, semantics, and
diagnostics.

## Lexical Grammar

```ebnf
letter        = "A".."Z" | "a".."z" | "_" ;
digit         = "0".."9" ;
hex_digit     = digit | "A".."F" | "a".."f" ;

identifier    = letter, { letter | digit } ;

int_lit       = digit, { digit } ;
float_lit     = digit, { digit }, ".", digit, { digit } ;
number_lit    = float_lit | int_lit ;

string_char   = ? any character except quote, newline, or carriage return ? ;
string_lit    = '"', { string_char }, '"' ;

color_lit     = "#", ( hex_digit, hex_digit, hex_digit
                     | hex_digit, hex_digit, hex_digit,
                       hex_digit, hex_digit, hex_digit ) ;

duration_lit  = duration_part, { duration_part } ;
duration_part = number_lit, duration_unit ;
duration_unit = "ms" | "s" | "m" ;

path_char     = ? any character except "<", ">", newline, or carriage return ? ;
path_lit      = "<", path_char, { path_char }, ">" ;

ipv4_octet    = digit, { digit } ;
address_lit   = ipv4_octet, ".", ipv4_octet, ".", ipv4_octet, ".", ipv4_octet ;

line_comment  = "//", ? any characters until newline ? ;
```

## Source File Grammar

```ebnf
source_file    = { import_decl }, document ;

document       = project_doc
               | display_doc
               | hardware_doc
               | layout_doc
               | patch_doc
               | sequence_doc
               | curve_doc
               | effect_doc ;

doc_name       = identifier ;
version_decl   = "version", int_lit, ";" ;
label_decl     = "label", string_lit, ";" ;
description_decl = "description", string_lit, ";" ;
display_text_decl = label_decl | description_decl ;

import_decl    = "import", doc_kind, doc_name, "from", path_lit, ";" ;
doc_kind       = "project" | "display" | "sequence" | "hardware"
               | "layout" | "patch" | "curve" | "effect" ;
```

## Document Grammar

```ebnf
project_doc    = "project", doc_name, "{",
                   version_decl,
                   { display_text_decl | project_field },
                 "}" ;

project_field  = "display", qualified_name, ";"
               | "sequence", qualified_name, ";" ;

display_doc    = "display", doc_name, "{",
                   version_decl,
                   { display_text_decl | display_field },
                 "}" ;

display_field  = "hardware", qualified_name, ";"
               | "layout", qualified_name, ";"
               | "patch", qualified_name, ";" ;

hardware_doc   = "hardware", doc_name, "{",
                   version_decl,
                   { display_text_decl | controller_decl },
                 "}" ;

controller_decl = "controller", doc_name, "{",
                    { display_text_decl },
                    "vendor", identifier, ";",
                    "model", identifier, ";",
                    [ "variant", identifier, ";" ],
                    { input_decl },
                    { output_decl },
                  "}" ;

input_decl     = "input", doc_name, "{",
                   { display_text_decl },
                   "type", input_type, ";",
                   input_body,
                 "}" ;
input_type     = "e131" | "artnet" | "ddp" | "serial" ;
input_body     = e131_input | artnet_input | ddp_input | serial_input ;

e131_input     = "port", int_lit, ";",
                 [ "priority", int_lit, ";" ],
                 [ "sync_universe", int_lit, ";" ],
                 { e131_universe } ;

e131_universe  = "universe", doc_name, "{",
                   { display_text_decl },
                   "id", int_lit, ";",
                   "start_channel", int_lit, ";",
                   "channels", int_lit, ";",
                   "address", address_lit, ";",
                   "cast", identifier, ";",
                   [ "enabled", bool_lit, ";" ],
                 "}" ;

artnet_input   = "port", int_lit, ";",
                 { artnet_universe } ;

artnet_universe = "universe", doc_name, "{",
                    { display_text_decl },
                    "id", int_lit, ";",
                    [ "net", int_lit, ";" ],
                    [ "subnet", int_lit, ";" ],
                    "start_channel", int_lit, ";",
                    "channels", int_lit, ";",
                    "address", address_lit, ";",
                    "cast", identifier, ";",
                    [ "enabled", bool_lit, ";" ],
                  "}" ;

ddp_input      = "address", address_lit, ";",
                 "port", int_lit, ";",
                 "destination", int_lit, ";",
                 "channel_count", int_lit, ";",
                 [ "channels_per_packet", int_lit, ";" ],
                 [ "keep_channel_numbers", bool_lit, ";" ],
                 [ "byte_offset", int_lit, ";" ] ;

serial_input   = "device", path_lit, ";",
                 "baud", int_lit, ";",
                 "data_bits", int_lit, ";",
                 "parity", identifier, ";",
                 "stop_bits", int_lit, ";",
                 "protocol", serial_protocol, ";",
                 "channel_count", int_lit, ";" ;
serial_protocol = "dmx" | "opendmx" | "renard" | "lor" | "dlight"
                | "pixelnet" | "generic_serial" ;

output_decl    = "output", doc_name, "{",
                   { display_text_decl },
                   "type", output_type, ";",
                   output_common,
                   output_body,
                 "}" ;
output_type    = "pixel_string" | "dmx" | "dumb_rgb" | "dumb_rgbw"
               | "single_channel" ;
output_common  = "input", qualified_name, ";",
                 "input_channel", int_lit, ";",
                 "connector", identifier, ";",
                 "index", int_lit, ";" ;
output_body    = pixel_string_output | dmx_output | dumb_output
               | single_channel_output ;

pixel_string_output = "pixel_protocol", identifier, ";",
                      "node_count", int_lit, ";",
                      "color_order", identifier, ";",
                      "white_mode", identifier, ";",
                      "null_nodes", int_lit, ";",
                      "end_null_nodes", int_lit, ";",
                      "group_size", int_lit, ";",
                      "reverse", bool_lit, ";",
                      "zigzag", int_lit, ";",
                      "brightness", number_lit, ";",
                      "gamma", number_lit, ";",
                      { color_order_override | virtual_string_decl } ;
color_order_override = "color_order_override", int_lit, int_lit,
                       identifier, ";" ;

virtual_string_decl = "virtual_string", doc_name, "{",
                        { display_text_decl },
                        "input_channel", int_lit, ";",
                        "node_count", int_lit, ";",
                        { virtual_string_override },
                      "}" ;
virtual_string_override = "color_order", identifier, ";"
                        | "white_mode", identifier, ";"
                        | "null_nodes", int_lit, ";"
                        | "end_null_nodes", int_lit, ";"
                        | "group_size", int_lit, ";"
                        | "reverse", bool_lit, ";"
                        | "zigzag", int_lit, ";"
                        | "brightness", number_lit, ";"
                        | "gamma", number_lit, ";" ;

dmx_output     = [ "fixture_profile", identifier, ";" ],
                 ( dmx_raw_output | dmx_fixture_bank_output ) ;
dmx_raw_output = "channel_count", int_lit, ";" ;
dmx_fixture_bank_output = "fixture_count", int_lit, ";",
                          "color_order", identifier, ";" ;
dumb_output    = "fixture_count", int_lit, ";",
                 "color_order", identifier, ";" ;
single_channel_output = "channel_count", int_lit, ";" ;

layout_doc     = "layout", doc_name, "{",
                   version_decl,
                   { display_text_decl | fixture_decl | group_decl },
                 "}" ;

fixture_decl   = "fixture", doc_name, "{",
                   { display_text_decl },
                   "shape", shape_decl, ";",
                   "pixel_count", int_lit, ";",
                 "}" ;

shape_decl     = line_shape | grid_shape ;
line_shape     = "line", "(", "start", coord, ",", "end", coord, ")" ;
grid_shape     = "grid", "(", "top_left", coord, ",", "bottom_right", coord,
                 ",", "columns", int_lit, ")" ;
coord          = "(", number_lit, ",", number_lit, ")" ;

group_decl     = "group", doc_name, "{",
                   { display_text_decl },
                   "members", "[", qualified_name,
                   { ",", qualified_name }, "]", ";",
                 "}" ;

patch_doc      = "patch", doc_name, "{",
                   version_decl,
                   { display_text_decl | filter_decl | route_decl },
                 "}" ;

filter_decl    = "filter", doc_name, "{",
                   { display_text_decl },
                   "type", identifier, ";",
                   [ "order", identifier, ";" ],
                 "}" ;

route_decl     = "route", doc_name, "{",
                   { display_text_decl },
                   "from", route_endpoint, ";",
                   route_mapping,
                 "}" ;
route_mapping  = full_span_route | explicit_span_route ;
full_span_route = "to", route_endpoint, ";" ;
explicit_span_route = "from_channel", int_lit, ";",
                      "to", route_endpoint, ";",
                      "to_channel", int_lit, ";",
                      "channels", int_lit, ";" ;
route_endpoint = qualified_name ;

sequence_doc   = "sequence", doc_name, "{",
                   version_decl,
                   { display_text_decl | sequence_field | event_decl },
                 "}" ;

sequence_field = "duration", duration_lit, ";"
               | "frame_rate", number_lit, ";"
               | "audio", path_lit, ";" ;

event_decl     = "event", doc_name, "{",
                   { display_text_decl },
                   "at", duration_lit, "for", duration_lit, ";",
                   effect_call_stmt,
                 "}" ;

effect_call_stmt = "effect", qualified_name, "(", [ named_arg_list ], ")", ";" ;
named_arg_list   = named_arg, { ",", named_arg } ;
named_arg        = identifier, value_expr ;

curve_doc      = "curve", doc_name, "{",
                   version_decl,
                   { display_text_decl },
                   "range", curve_value_type, ";",
                   { curve_key_decl },
                 "}" ;

curve_key_decl = "key", number_lit, curve_key_value,
                 [ identifier ], ";" ;

curve_key_value = number_lit | color_lit | bool_lit | qualified_name ;

curve_value_type = ranged_float_type
                 | ranged_int_type
                 | "float"
                 | "int"
                 | "bool"
                 | "color" ;

effect_doc     = "effect", doc_name, "{",
                   version_decl,
                   { display_text_decl },
                   [ "spatial", bool_lit, ";" ],
                   { enum_decl | flags_decl | param_decl | fn_decl | let_decl },
                   expr,
                 "}" ;

enum_decl      = "enum", identifier, "{", identifier,
                 { ",", identifier }, "}" ;

flags_decl     = "flags", identifier, "{", identifier,
                 { ",", identifier }, "}" ;

param_decl     = "param", identifier, ":", param_type,
                 [ "=", value_expr ], ";" ;

param_type     = ranged_float_type
               | ranged_int_type
               | "bool"
               | "color"
               | curve_type
               | identifier ;

curve_type     = "curve", [ "<", curve_value_type, ">" ] ;

ranged_float_type = "float", "(", float_lit, ",", float_lit, ")" ;
ranged_int_type   = "int", "(", int_lit, ",", int_lit, ")" ;

fn_decl        = "fn", identifier, "(", [ fn_param_list ], ")",
                 ":", type_name, "{", expr, "}" ;
fn_param_list  = fn_param, { ",", fn_param } ;
fn_param       = identifier, ":", type_name ;

let_decl       = "let", identifier, "=", expr, ";" ;
```

## Value Grammar

```ebnf
value_expr     = bool_lit
               | number_lit
               | color_lit
               | duration_lit
               | address_lit
               | path_lit
               | qualified_name ;

bool_lit       = "true" | "false" ;
qualified_name = identifier, { ".", identifier } ;
type_name      = identifier ;
```

`string_lit` is intentionally absent from `value_expr`. It is valid only in
`label` and `description` declarations.

`route_endpoint` uses `qualified_name` syntax. Endpoint roots named `hardware`
and `layout` are contextual roots supplied by the owning display, not imports
inside the patch document.

## Effect Expression Grammar

```ebnf
expr           = if_expr | switch_expr | logical_or ;

if_expr        = "if", expr, "{", expr, "}",
                 "else", "{", expr, "}" ;

switch_expr    = "switch", expr, "{",
                   { "case", pattern, ":", expr, ";" },
                   "default", ":", expr, ";",
                 "}" ;

pattern        = qualified_name | number_lit | bool_lit ;

logical_or     = logical_and, { "||", logical_and } ;
logical_and    = bitwise_or, { "&&", bitwise_or } ;
bitwise_or     = bitwise_xor, { "|", bitwise_xor } ;
bitwise_xor    = bitwise_and, { "^", bitwise_and } ;
bitwise_and    = equality, { "&", equality } ;
equality       = comparison, { ( "==" | "!=" ), comparison } ;
comparison     = additive, { ( "<" | "<=" | ">" | ">=" ), additive } ;
additive       = multiplicative, { ( "+" | "-" ), multiplicative } ;
multiplicative = exponent, { ( "*" | "/" | "%" ), exponent } ;
exponent       = unary, [ "**", exponent ] ;
unary          = ( "!" | "-" ), unary | postfix ;

postfix        = primary, { call_suffix | field_suffix } ;
call_suffix    = "(", [ expr_list ], ")" ;
field_suffix   = ".", identifier ;
expr_list      = expr, { ",", expr } ;

primary        = number_lit
               | color_lit
               | bool_lit
               | qualified_name
               | "(", expr, ")" ;
```

`**` is right-associative. Binary operators at the same precedence level are
left-associative.
