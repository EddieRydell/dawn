use camino::Utf8PathBuf;
use dawn_project_io::load_project;

use crate::{PreparedRenderSession, RenderedElementState};

fn example(name: &str) -> dawn_project_io::ProjectSession {
    let path = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("examples")
        .join(name)
        .join("project.dawn");
    load_project(&path).unwrap_or_else(|error| panic!("failed to load {name}: {error}"))
}

#[test]
fn every_example_prepares_and_produces_exact_controller_widths() {
    for name in ["starter", "christmas-house", "thirty-output-controller"] {
        let session = example(name);
        let sequence_id = session.project.root.sequences.first().unwrap();
        let renderer = PreparedRenderSession::prepare(
            &session.project,
            &session.project.root.setup,
            sequence_id,
        )
        .unwrap_or_else(|error| panic!("failed to prepare {name}: {error:?}"));
        let frame = renderer
            .render_seconds(0.0)
            .unwrap_or_else(|error| panic!("failed to render {name}: {error:?}"));
        let setup = session
            .project
            .setups
            .get(&session.project.root.setup)
            .unwrap();
        let expected_ports = setup
            .controllers
            .iter()
            .map(|id| session.project.controllers.get(id).unwrap().ports.len())
            .sum::<usize>();
        assert_eq!(frame.controller_frames.len(), expected_ports);
        for port_frame in &frame.controller_frames {
            let controller = session
                .project
                .controllers
                .get(&port_frame.controller)
                .unwrap();
            let port = controller
                .ports
                .iter()
                .find(|port| port.id == port_frame.port)
                .unwrap();
            assert_eq!(port_frame.slots.len(), usize::from(port.slot_count));
        }
    }
}

#[test]
fn preview_and_controller_buffers_are_from_one_deterministic_show_frame() {
    let session = example("thirty-output-controller");
    let sequence_id = session.project.root.sequences.first().unwrap();
    let renderer =
        PreparedRenderSession::prepare(&session.project, &session.project.root.setup, sequence_id)
            .unwrap();
    let first = renderer.render_frame(10).unwrap();
    let second = renderer.render_frame(10).unwrap();
    assert_eq!(first, second);
    assert!(!first.elements.is_empty());
    assert!(!first.controller_frames.is_empty());
    let preview_checksum = first.elements.iter().fold(0u64, |hash, element| {
        element
            .preview_colors()
            .into_iter()
            .fold(hash, |hash, color| {
                hash.wrapping_mul(16777619)
                    ^ u64::from(color.red)
                    ^ (u64::from(color.green) << 8)
                    ^ (u64::from(color.blue) << 16)
            })
    });
    let controller_checksum = first.controller_frames.iter().fold(0u64, |hash, frame| {
        frame.slots.iter().fold(hash, |hash, slot| {
            hash.wrapping_mul(16777619) ^ u64::from(*slot)
        })
    });
    assert_eq!(
        preview_checksum,
        second.elements.iter().fold(0u64, |hash, element| {
            element
                .preview_colors()
                .into_iter()
                .fold(hash, |hash, color| {
                    hash.wrapping_mul(16777619)
                        ^ u64::from(color.red)
                        ^ (u64::from(color.green) << 8)
                        ^ (u64::from(color.blue) << 16)
                })
        })
    );
    assert_eq!(
        controller_checksum,
        second
            .controller_frames
            .iter()
            .fold(0u64, |hash, frame| frame
                .slots
                .iter()
                .fold(hash, |hash, slot| hash.wrapping_mul(16777619)
                    ^ u64::from(*slot)))
    );
}

#[test]
fn logical_state_covers_every_element_leaf_in_tree_order() {
    let session = example("christmas-house");
    let sequence_id = session.project.root.sequences.first().unwrap();
    let renderer =
        PreparedRenderSession::prepare(&session.project, &session.project.root.setup, sequence_id)
            .unwrap();
    let frame = renderer.render_seconds(1.0).unwrap();
    let setup = session
        .project
        .setups
        .get(&session.project.root.setup)
        .unwrap();
    let tree = session.project.element_trees.get(&setup.elements).unwrap();
    let expected = tree
        .nodes
        .values()
        .filter(|node| {
            !matches!(
                node.kind,
                dawn_language::element::ElementNodeKind::Group { .. }
            )
        })
        .count();
    assert_eq!(frame.elements.len(), expected);
    assert!(frame.elements.iter().all(|state| matches!(
        state,
        RenderedElementState::Color { .. }
            | RenderedElementState::Scalar { .. }
            | RenderedElementState::Indexed { .. }
            | RenderedElementState::Fixture { .. }
    )));
}
