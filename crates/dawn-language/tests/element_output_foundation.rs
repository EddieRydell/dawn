use dawn_language::controller::{ControllerId, ControllerPortId};
use dawn_language::element::{
    ColorCapability, ElementCellRange, ElementNode, ElementNodeId, ElementNodeKind,
    ElementSelection, ElementTree, ElementTreeId, ElementTreeValidationError,
};
use dawn_language::fixture_profile::FixtureProfileStore;
use dawn_language::identity::{DocumentId, SourceIdentity};
use dawn_language::patch::{
    ByteOrder, FilterDefinition, PatchEdge, PatchGraph, PatchId, PatchNode, PatchNodeId,
    PatchPortId, PatchSink, PatchSource, PatchValidationError, PatchValue, PatchValueType,
    evaluate_filter,
};
use dawn_language::values::Color;
use indexmap::IndexMap;

fn identity(object: &str) -> SourceIdentity {
    SourceIdentity::from_document(
        DocumentId::new(
            uuid::Uuid::from_u128(0x00000000000040008000000000000002),
            "foundation.dawn".into(),
        ),
        object.to_string(),
    )
}

#[test]
fn element_tree_rejects_cycles_and_flattens_declared_group_order() {
    let id = ElementTreeId(identity("elements"));
    let mut nodes = IndexMap::new();
    nodes.insert(
        ElementNodeId(1),
        ElementNode {
            name: "All".to_string(),
            kind: ElementNodeKind::Group {
                children: vec![ElementNodeId(3), ElementNodeId(2)],
            },
        },
    );
    nodes.insert(
        ElementNodeId(2),
        ElementNode {
            name: "A".to_string(),
            kind: ElementNodeKind::Color {
                cells: 2,
                capability: ColorCapability::Rgb,
            },
        },
    );
    nodes.insert(
        ElementNodeId(3),
        ElementNode {
            name: "B".to_string(),
            kind: ElementNodeKind::Scalar { cells: 1 },
        },
    );
    let tree = ElementTree {
        id: id.clone(),
        roots: vec![ElementNodeId(1)],
        nodes,
    };
    tree.validate().unwrap();
    let addresses = tree
        .flatten_selection(&ElementSelection {
            tree: id,
            node: ElementNodeId(1),
            cells: None,
        })
        .unwrap();
    assert_eq!(
        addresses
            .iter()
            .map(|address| (address.node.0, address.cell))
            .collect::<Vec<_>>(),
        vec![(3, 0), (2, 0), (2, 1)]
    );

    let mut cyclic = tree.clone();
    cyclic.nodes.insert(
        ElementNodeId(3),
        ElementNode {
            name: "cycle".to_string(),
            kind: ElementNodeKind::Group {
                children: vec![ElementNodeId(1)],
            },
        },
    );
    assert!(matches!(
        cyclic.validate(),
        Err(ElementTreeValidationError::RootHasParent(_) | ElementTreeValidationError::Cycle(_))
    ));
}

#[test]
fn element_cell_ranges_are_zero_based_exact_and_checked() {
    let id = ElementTreeId(identity("elements"));
    let tree = ElementTree {
        id: id.clone(),
        roots: vec![ElementNodeId(1)],
        nodes: IndexMap::from([(
            ElementNodeId(1),
            ElementNode {
                name: "Pixels".to_string(),
                kind: ElementNodeKind::Color {
                    cells: 4,
                    capability: ColorCapability::Rgb,
                },
            },
        )]),
    };
    let addresses = tree
        .flatten_selection(&ElementSelection {
            tree: id.clone(),
            node: ElementNodeId(1),
            cells: Some(ElementCellRange { start: 1, count: 2 }),
        })
        .unwrap();
    assert_eq!(
        addresses
            .iter()
            .map(|address| address.cell)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert!(
        tree.flatten_selection(&ElementSelection {
            tree: id,
            node: ElementNodeId(1),
            cells: Some(ElementCellRange { start: 3, count: 2 })
        })
        .is_err()
    );
}

#[test]
fn rgbw_breakdown_extracts_minimum_rgb_as_white() {
    let output = evaluate_filter(
        &FilterDefinition::ColorBreakdown {
            capability: ColorCapability::Rgbw,
            cell_count: 1,
        },
        &PatchValue::Colors(vec![Color {
            red: 128,
            green: 64,
            blue: 96,
        }]),
        &FixtureProfileStore::default(),
    )
    .unwrap();
    let PatchValue::Components(values) = &output[0] else {
        panic!("expected components");
    };
    let white = 64.0 / 255.0;
    assert!((values[0] - (128.0 / 255.0 - white)).abs() < 1e-9);
    assert!((values[1] - 0.0).abs() < 1e-9);
    assert!((values[2] - (96.0 / 255.0 - white)).abs() < 1e-9);
    assert!((values[3] - white).abs() < 1e-9);
}

#[test]
fn quantizers_cover_boundaries_and_explicit_byte_order() {
    let profiles = FixtureProfileStore::default();
    let eight = evaluate_filter(
        &FilterDefinition::Quantize8 { width: 3 },
        &PatchValue::Components(vec![0.0, 0.5, 1.0]),
        &profiles,
    )
    .unwrap();
    assert_eq!(eight, vec![PatchValue::Slots(vec![0, 128, 255])]);
    let sixteen = evaluate_filter(
        &FilterDefinition::Quantize16 {
            width: 2,
            byte_order: ByteOrder::FineCoarse,
        },
        &PatchValue::Components(vec![0.0, 1.0]),
        &profiles,
    )
    .unwrap();
    assert_eq!(sixteen, vec![PatchValue::Slots(vec![0, 0, 255, 255])]);
}

#[test]
fn patch_graph_rejects_overlapping_controller_destinations() {
    let controller = ControllerId(identity("controller"));
    let tree = ElementTreeId(identity("elements"));
    let mut nodes = IndexMap::new();
    for (base, start) in [(1, 0), (4, 2)] {
        nodes.insert(
            PatchNodeId(base),
            PatchNode::Source(PatchSource {
                selection: ElementSelection {
                    tree: tree.clone(),
                    node: ElementNodeId(base),
                    cells: None,
                },
                output: PatchValueType::Components { width: 3 },
            }),
        );
        nodes.insert(
            PatchNodeId(base + 1),
            PatchNode::Filter(FilterDefinition::Quantize8 { width: 3 }),
        );
        nodes.insert(
            PatchNodeId(base + 2),
            PatchNode::Sink(PatchSink {
                controller: controller.clone(),
                port: ControllerPortId(1),
                start_slot: start,
                slot_count: 3,
            }),
        );
    }
    let edges = vec![
        PatchEdge {
            from: PatchNodeId(1),
            from_port: PatchPortId(0),
            to: PatchNodeId(2),
            to_port: PatchPortId(0),
        },
        PatchEdge {
            from: PatchNodeId(2),
            from_port: PatchPortId(0),
            to: PatchNodeId(3),
            to_port: PatchPortId(0),
        },
        PatchEdge {
            from: PatchNodeId(4),
            from_port: PatchPortId(0),
            to: PatchNodeId(5),
            to_port: PatchPortId(0),
        },
        PatchEdge {
            from: PatchNodeId(5),
            from_port: PatchPortId(0),
            to: PatchNodeId(6),
            to_port: PatchPortId(0),
        },
    ];
    let graph = PatchGraph {
        id: PatchId(identity("patch")),
        nodes,
        edges,
    };
    assert!(matches!(
        graph.validate(),
        Err(PatchValidationError::DestinationOverlap { .. })
    ));
}
