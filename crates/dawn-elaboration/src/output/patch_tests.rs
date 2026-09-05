use super::*;
use camino::Utf8PathBuf;
use dawn_language::element::{ColorCapability, ElementNodeKind};
use dawn_language::patch::PatchEdge;
use dawn_language::values::Color;
use dawn_project_io::load_package;
use dawn_runtime::element::RenderedElementState;
use dawn_runtime::patch::{PatchValue, PatchValueLayout, PreparedFilter};

#[test]
fn reorders_are_composed_into_packing_without_changing_output() {
    let path = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter");
    let project = load_package(&path).unwrap().session.project;
    let setup = &project.setups[&project.root.setup];
    let tree = &project.element_trees[&setup.elements];
    let profiles = &project.definitions.fixture_profiles;
    let frames = setup
        .controllers
        .iter()
        .flat_map(|id| {
            project.controllers[id]
                .ports
                .iter()
                .map(|port| ControllerPortFrame {
                    controller: id.clone(),
                    port: port.id,
                    slots: vec![0; usize::from(port.slot_count)],
                })
        })
        .collect::<Vec<_>>();
    let mut patch = project.patches[&setup.patch].clone();
    let before = prepare_patch(tree, &patch, profiles, &frames).unwrap();
    let element_ids = tree
        .nodes
        .iter()
        .filter_map(|(id, node)| {
            (!matches!(node.kind, ElementNodeKind::Group { .. })).then_some(*id)
        })
        .collect::<Vec<_>>();
    let source = before
        .steps
        .iter()
        .find_map(|step| match step {
            PatchStep::Source { source, .. } => Some(source),
            _ => None,
        })
        .unwrap();
    let authored = patch
        .nodes
        .values()
        .find_map(|node| match node {
            PatchNode::Source(source) => Some(source),
            _ => None,
        })
        .unwrap();
    let expected_addresses = tree
        .flatten_selection(&authored.selection)
        .unwrap()
        .into_iter()
        .map(|address| (address.node, address.cell))
        .collect::<Vec<_>>();
    let actual_addresses = source
        .spans
        .iter()
        .flat_map(|span| {
            span.cells
                .clone()
                .map(|cell| (element_ids[span.element as usize], cell))
        })
        .collect::<Vec<_>>();
    assert_eq!(actual_addresses, expected_addresses);
    assert!(source.spans.len() < actual_addresses.len());
    assert!(
        source
            .spans
            .windows(2)
            .all(|pair| pair[0].element != pair[1].element
                || pair[0].cells.end != pair[1].cells.start),
        "adjacent source runs must already be merged"
    );
    assert_eq!(
        before.value_layouts.as_ref(),
        &[PatchValueLayout::Color(113), PatchValueLayout::Slots(339)]
    );
    let (source, cell_count) = patch
        .nodes
        .iter()
        .find_map(|(id, node)| match node {
            PatchNode::Filter(FilterDefinition::ColorBreakdown {
                capability: ColorCapability::Rgb,
                cell_count,
            }) => Some((*id, *cell_count)),
            _ => None,
        })
        .expect("starter has an RGB output path");
    let added = PatchNodeId(patch.nodes.keys().map(|id| id.0).max().unwrap() + 1);
    for edge in &mut patch.edges {
        if edge.from == source {
            edge.from = added;
        }
    }
    patch.edges.push(PatchEdge {
        from: source,
        from_port: PatchPortId(0),
        to: added,
        to_port: PatchPortId(0),
    });
    patch.nodes.insert(
        added,
        PatchNode::Filter(FilterDefinition::ComponentReorder {
            components_per_cell: 3,
            order: vec![0, 1, 2],
            cell_count,
        }),
    );
    let identity = prepare_patch(tree, &patch, profiles, &frames).unwrap();
    assert_eq!(identity.steps.len(), before.steps.len());
    assert_eq!(identity.value_layouts, before.value_layouts);
    assert_eq!(identity.fixture_programs, before.fixture_programs);
    let elements = tree
        .nodes
        .iter()
        .filter_map(|(id, node)| match &node.kind {
            ElementNodeKind::Group { .. } => None,
            ElementNodeKind::Color { cells, .. } => Some(RenderedElementState::Color {
                node: *id,
                cells: (0..*cells)
                    .map(|cell| Color {
                        red: id.0.wrapping_mul(17) as u8,
                        green: cell as u8,
                        blue: id.0.wrapping_mul(31).wrapping_add(cell) as u8,
                    })
                    .collect(),
            }),
            _ => panic!("starter patch fixture expects color elements"),
        })
        .collect::<Vec<_>>();
    let mut expected = frames.clone();
    before
        .evaluate(&elements, &mut expected, &mut before.workspace())
        .unwrap();
    let mut actual = frames.clone();
    identity
        .evaluate(&elements, &mut actual, &mut identity.workspace())
        .unwrap();
    assert_eq!(actual, expected);

    let PatchNode::Filter(FilterDefinition::ComponentReorder { order, .. }) =
        patch.nodes.get_mut(&added).unwrap()
    else {
        unreachable!()
    };
    order.reverse();
    let reordered = prepare_patch(tree, &patch, profiles, &frames).unwrap();
    assert_eq!(reordered.steps.len(), before.steps.len());
    assert!(reordered.steps.iter().any(|step| matches!(step,
        PatchStep::Filter { filter: PreparedFilter::PackRgb { order, .. }, .. } if *order == [1, 2, 0]
    )));
    reordered
        .evaluate(&elements, &mut actual, &mut reordered.workspace())
        .unwrap();
    assert_ne!(actual, expected);
}

#[test]
fn rgb_packing_is_exact_for_every_byte_and_preserves_shared_or_transformed_inputs() {
    let colors = (0..=255u8)
        .map(|value| Color {
            red: value,
            green: !value,
            blue: value.rotate_left(1),
        })
        .collect::<Vec<_>>();
    for order in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        let original = vec![
            PatchStep::Filter {
                input: 0,
                output_start: 1,
                filter: PreparedFilter::ColorBreakdown {
                    capability: ColorEncoding::Rgb,
                    cell_count: 256,
                },
            },
            PatchStep::Filter {
                input: 1,
                output_start: 2,
                filter: PreparedFilter::ComponentReorder {
                    components_per_cell: 3,
                    order: Box::new(order),
                    cell_count: 256,
                },
            },
            PatchStep::Filter {
                input: 2,
                output_start: 3,
                filter: PreparedFilter::Quantize8 { width: 768 },
            },
        ];
        let mut expected = PatchValue::Colors(colors.clone());
        for step in &original {
            let PatchStep::Filter { filter, .. } = step else {
                unreachable!()
            };
            let layout = if matches!(filter, PreparedFilter::Quantize8 { .. }) {
                PatchValueLayout::Slots(768)
            } else {
                PatchValueLayout::Components(768)
            };
            let mut output = [PatchValue::new(layout)];
            filter.evaluate(&expected, &mut output).unwrap();
            [expected] = output;
        }
        let mut packed = original.clone();
        fuse_rgb_packing(&mut packed, 4).unwrap();
        assert_eq!(packed.len(), 1);
        let PatchStep::Filter {
            input: 0,
            output_start: 3,
            filter: filter @ PreparedFilter::PackRgb { .. },
        } = &packed[0]
        else {
            panic!("expected packed RGB")
        };
        let mut actual = [PatchValue::new(PatchValueLayout::Slots(768))];
        filter
            .evaluate(&PatchValue::Colors(colors.clone()), &mut actual)
            .unwrap();
        assert_eq!(actual[0], expected);

        let mut shared = original.clone();
        shared.push(PatchStep::Filter {
            input: 1,
            output_start: 4,
            filter: PreparedFilter::Quantize8 { width: 768 },
        });
        fuse_rgb_packing(&mut shared, 5).unwrap();
        assert_eq!(
            shared.len(),
            4,
            "shared component values must not be removed"
        );

        let mut transformed = original;
        transformed.insert(
            1,
            PatchStep::Filter {
                input: 1,
                output_start: 4,
                filter: PreparedFilter::DimmingCurve {
                    curve: dawn_language::fixture_profile::DimmingCurve::Gamma(2.0),
                    width: 768,
                },
            },
        );
        let PatchStep::Filter { input, .. } = &mut transformed[2] else {
            unreachable!()
        };
        *input = 4;
        let expected = evaluate_rgb_chain(&transformed, &colors);
        fuse_rgb_packing(&mut transformed, 5).unwrap();
        assert_eq!(transformed.len(), 1);
        assert_eq!(evaluate_rgb_chain(&transformed, &colors), expected);
    }
    let mut rgbw = vec![
        PatchStep::Filter {
            input: 0,
            output_start: 1,
            filter: PreparedFilter::ColorBreakdown {
                capability: ColorEncoding::Rgbw,
                cell_count: 256,
            },
        },
        PatchStep::Filter {
            input: 1,
            output_start: 2,
            filter: PreparedFilter::Quantize8 { width: 1024 },
        },
    ];
    fuse_rgb_packing(&mut rgbw, 3).unwrap();
    assert_eq!(rgbw.len(), 2, "RGBW needs white extraction");
}

fn evaluate_rgb_chain(steps: &[PatchStep], colors: &[Color]) -> PatchValue {
    let mut input = PatchValue::Colors(colors.to_vec());
    for step in steps {
        let PatchStep::Filter { filter, .. } = step else {
            unreachable!()
        };
        let layout = if matches!(
            filter,
            PreparedFilter::Quantize8 { .. } | PreparedFilter::PackRgb { .. }
        ) {
            PatchValueLayout::Slots(768)
        } else {
            PatchValueLayout::Components(768)
        };
        let mut output = [PatchValue::new(layout)];
        filter.evaluate(&input, &mut output).unwrap();
        [input] = output;
    }
    input
}

#[test]
fn rgb_lookup_preserves_transform_order_and_all_channel_values() {
    use dawn_language::fixture_profile::DimmingCurve;
    use dawn_language::values::{Curve, CurvePoint};
    let colors = (0..=255u8)
        .map(|value| Color {
            red: value,
            green: !value,
            blue: value.rotate_left(1),
        })
        .collect::<Vec<_>>();
    for curve in [
        DimmingCurve::Linear,
        DimmingCurve::Gamma(0.5),
        DimmingCurve::Gamma(2.2),
        DimmingCurve::Custom(Curve {
            points: vec![
                CurvePoint {
                    position: 0.0,
                    value: 0.9,
                },
                CurvePoint {
                    position: 0.4,
                    value: 0.2,
                },
                CurvePoint {
                    position: 1.0,
                    value: 0.7,
                },
            ],
        }),
    ] {
        for order in [
            [0, 1, 2],
            [0, 2, 1],
            [1, 0, 2],
            [1, 2, 0],
            [2, 0, 1],
            [2, 1, 0],
        ] {
            let filters = [
                PreparedFilter::ColorBreakdown {
                    capability: ColorEncoding::Rgb,
                    cell_count: 256,
                },
                PreparedFilter::ScaleInvert {
                    scale: 0.8,
                    invert: false,
                    width: 768,
                },
                PreparedFilter::DimmingCurve {
                    curve: curve.clone(),
                    width: 768,
                },
                PreparedFilter::ComponentReorder {
                    components_per_cell: 3,
                    order: Box::new(order),
                    cell_count: 256,
                },
                PreparedFilter::ScaleInvert {
                    scale: 1.3,
                    invert: true,
                    width: 768,
                },
                PreparedFilter::DimmingCurve {
                    curve: DimmingCurve::Gamma(1.7),
                    width: 768,
                },
                PreparedFilter::Quantize8 { width: 768 },
            ];
            let mut steps = filters
                .into_iter()
                .enumerate()
                .map(|(index, filter)| PatchStep::Filter {
                    input: index as u32,
                    output_start: index as u32 + 1,
                    filter,
                })
                .collect::<Vec<_>>();
            let expected = evaluate_rgb_chain(&steps, &colors);
            fuse_rgb_packing(&mut steps, 8).unwrap();
            assert_eq!(steps.len(), 1);
            assert_eq!(evaluate_rgb_chain(&steps, &colors), expected);
        }
    }
}
