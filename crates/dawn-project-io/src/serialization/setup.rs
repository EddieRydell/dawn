use super::*;

pub(super) fn setup_value(
    session: &ProjectSession,
    from_document: &Utf8Path,
    setup: &Setup,
) -> Result<Value, ExportProjectError> {
    let mut value = typed_object("setup");
    value.insert(
        string_value("layout"),
        Value::String(write_source_reference(
            session,
            from_document,
            SourceObjectKind::Layout,
            &setup.layout.0,
        )?),
    );
    value.insert(
        string_value("patch"),
        Value::String(write_source_reference(
            session,
            from_document,
            SourceObjectKind::Patch,
            &setup.patch.0,
        )?),
    );
    value.insert(
        string_value("controllers"),
        Value::Sequence(
            setup
                .controllers
                .iter()
                .map(|controller| Value::String(controller.0.object().to_string()))
                .collect(),
        ),
    );
    Ok(Value::Mapping(value))
}

pub(super) fn controller_value(
    controller: &ControllerDefinition,
) -> Result<Value, ExportProjectError> {
    let mut value = typed_object("controller");
    value.insert(
        string_value("protocol"),
        Value::String(
            match controller.protocol {
                Protocol::E131 => "sacn",
                Protocol::Artnet => "artnet",
            }
            .to_string(),
        ),
    );
    if let Some(address) = &controller.address {
        value.insert(
            string_value("destination"),
            Value::String(format!("{}:{}", address.ip, address.port)),
        );
    }
    let Some(first) = controller.outputs.first() else {
        value.insert(string_value("output"), Value::Mapping(Mapping::new()));
        return Ok(Value::Mapping(value));
    };
    let linear = controller
        .outputs
        .iter()
        .enumerate()
        .all(|(index, output)| {
            output.channel_order == first.channel_order
                && output.pixels == first.pixels
                && output.first_universe == first.first_universe + index as u32
        });
    let mut output = Mapping::new();
    output.insert(
        string_value("channel_order"),
        Value::String(channel_order_name(&first.channel_order).to_string()),
    );
    if linear {
        output.insert(
            string_value("type"),
            Value::String("linear_rgb".to_string()),
        );
        output.insert(
            string_value("output_count"),
            number_value(controller.outputs.len() as u32)?,
        );
        output.insert(
            string_value("pixels_per_output"),
            number_value(first.pixels as u32)?,
        );
        output.insert(
            string_value("first_universe"),
            number_value(first.first_universe)?,
        );
    } else {
        output.insert(
            string_value("type"),
            Value::String("patched_dmx".to_string()),
        );
        output.insert(
            string_value("universes"),
            Value::Sequence(
                controller
                    .outputs
                    .iter()
                    .map(|output| {
                        let mut universe = Mapping::new();
                        universe.insert(string_value("id"), number_value(output.first_universe)?);
                        universe.insert(
                            string_value("range"),
                            Value::String(format!("1..{}", output.pixels * 3)),
                        );
                        Ok(Value::Mapping(universe))
                    })
                    .collect::<Result<Vec<_>, ExportProjectError>>()?,
            ),
        );
    }
    value.insert(string_value("output"), Value::Mapping(output));
    Ok(Value::Mapping(value))
}

pub(super) fn layout_value(
    session: &ProjectSession,
    from_document: &Utf8Path,
    layout: &Layout,
) -> Result<Value, ExportProjectError> {
    let mut value = typed_object("layout");
    value.insert(
        string_value("target_order"),
        Value::Sequence(
            layout
                .target_order
                .iter()
                .map(layout_target_value)
                .collect::<Result<Vec<_>, _>>()?,
        ),
    );
    value.insert(
        string_value("fixtures"),
        Value::Sequence(
            layout
                .fixtures
                .iter()
                .map(|fixture| fixture_inst_value(session, from_document, fixture))
                .collect::<Result<Vec<_>, _>>()?,
        ),
    );
    value.insert(
        string_value("groups"),
        Value::Sequence(
            layout
                .groups
                .iter()
                .map(fixture_group_value)
                .collect::<Result<Vec<_>, _>>()?,
        ),
    );
    Ok(Value::Mapping(value))
}

pub(super) fn fixture_inst_value(
    session: &ProjectSession,
    from_document: &Utf8Path,
    fixture: &FixtureInst,
) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    value.insert(string_value("id"), number_value(fixture.id.0)?);
    value.insert(string_value("name"), Value::String(fixture.name.clone()));
    value.insert(
        string_value("fixture"),
        Value::String(write_source_reference(
            session,
            from_document,
            SourceObjectKind::FixtureDefinition,
            &fixture.definition.0,
        )?),
    );
    value.insert(string_value("transform"), transform_value(fixture)?);
    Ok(Value::Mapping(value))
}

pub(super) fn fixture_group_value(group: &FixtureGroup) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    value.insert(string_value("id"), number_value(group.id.0)?);
    value.insert(string_value("name"), Value::String(group.name.clone()));
    value.insert(
        string_value("members"),
        Value::Sequence(
            group
                .fixtures
                .iter()
                .map(|fixture| number_value(fixture.0))
                .collect::<Result<Vec<_>, _>>()?,
        ),
    );
    Ok(Value::Mapping(value))
}

pub(super) fn patch_value(
    session: &ProjectSession,
    from_document: &Utf8Path,
    patch: &Patch,
) -> Result<Value, ExportProjectError> {
    let mut value = typed_object("patch");
    value.insert(
        string_value("routes"),
        Value::Sequence(
            patch
                .routes
                .iter()
                .map(|route| patch_route_value(session, from_document, route))
                .collect::<Result<Vec<_>, _>>()?,
        ),
    );
    Ok(Value::Mapping(value))
}

pub(super) fn patch_route_value(
    session: &ProjectSession,
    from_document: &Utf8Path,
    route: &PatchRoute,
) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    value.insert(string_value("fixture"), number_value(route.fixture.0)?);
    value.insert(
        string_value("controller"),
        Value::String(write_source_reference(
            session,
            from_document,
            SourceObjectKind::Controller,
            &route.controller.0,
        )?),
    );
    value.insert(string_value("output"), number_value(route.output.0)?);
    value.insert(
        string_value("start_channel_offset"),
        number_value(route.start_channel_offset)?,
    );
    value.insert(
        string_value("fixture_pixel_start"),
        number_value(route.fixture_pixels.start)?,
    );
    value.insert(
        string_value("fixture_pixel_count"),
        number_value(route.fixture_pixels.count)?,
    );
    Ok(Value::Mapping(value))
}

pub(super) fn fixture_definition_value(
    definition: &FixtureDefinition,
) -> Result<Value, ExportProjectError> {
    let mut value = typed_object("fixture");
    value.insert(
        string_value("bulb_diameter"),
        number_value(distance_span_meters(definition.bulb_radius) * 2.0)?,
    );
    value.insert(
        string_value("geometry"),
        geometry_value(&definition.geometry)?,
    );
    Ok(Value::Mapping(value))
}
